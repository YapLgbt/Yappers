use std::collections::BTreeSet;
use yappers_netkit::model::*;
fn setup() -> (ed25519_dalek::SigningKey, ed25519_dalek::SigningKey, String) {
    let home = signing_key(&new_secret()).unwrap();
    let author = signing_key(&new_secret()).unwrap();
    let id = hex::encode(home.verifying_key().as_bytes());
    (home, author, id)
}
fn post() -> Action {
    Action::Post {
        body: "Hello neighbors".into(),
        topic: "Friends".into(),
        anonymous: true,
        display_name: "Anonymous neighbor".into(),
    }
}
#[test]
fn signatures_bind_every_field_and_reject_noncanonical_keys() {
    let (home, author, id) = setup();
    let event =
        SignedEvent::create(&author, id.clone(), "test/community".into(), 1, post()).unwrap();
    event.validate().unwrap();
    let mut bad = event.clone();
    bad.payload.community = "another/community".into();
    assert!(bad.validate().is_err());
    let mut bad = event.clone();
    bad.payload.author = bad.payload.author.to_uppercase();
    assert!(bad.validate().is_err());
    let envelope = Envelope::accept(event, &home).unwrap();
    envelope.validate().unwrap();
    let mut bad = envelope;
    bad.accepted_at -= 1;
    assert!(bad.validate().is_err());
}

#[test]
fn documented_v4_signing_vector_verifies() {
    let event: SignedEvent = serde_json::from_str(include_str!("fixtures/event-v4.json")).unwrap();
    event.validate().unwrap();
    assert_eq!(
        event.header().unwrap().action,
        Action::Erased {
            commitment: "ca6ac6d2af303ac6e565367c3648f6764212feb4119aa478516693ee62cd445e".into(),
            reply: false,
        }
    );
}
#[test]
fn anonymous_payload_omits_alias_and_utf16_limits_match_kotlin() {
    let (_, author, id) = setup();
    let invalid = Action::Post {
        body: "x".into(),
        topic: "Friends".into(),
        anonymous: true,
        display_name: "Private alias".into(),
    };
    assert!(SignedEvent::create(&author, id.clone(), "test".into(), 1, invalid).is_err());
    let valid = Action::Post {
        body: "🦊".repeat(250),
        topic: "Friends".into(),
        anonymous: false,
        display_name: "Neighbor".into(),
    };
    assert!(SignedEvent::create(&author, id.clone(), "test".into(), 2, valid).is_ok());
    let too_long = Action::Reply {
        parent: "a".repeat(64),
        body: "🦊".repeat(251),
        display_name: "Neighbor".into(),
    };
    assert!(SignedEvent::create(&author, id, "test".into(), 3, too_long).is_err());
}
#[test]
fn projection_converges_for_votes_replies_and_tombstones_in_any_arrival_order() {
    let (home, author, id) = setup();
    let make = |seq, action| {
        Envelope::accept(
            SignedEvent::create(&author, id.clone(), "test".into(), seq, action).unwrap(),
            &home,
        )
        .unwrap()
    };
    let post = make(1, post());
    let target = post.event.id.clone();
    let reply = make(
        2,
        Action::Reply {
            parent: target.clone(),
            body: "I'm in".into(),
            display_name: "Friend".into(),
        },
    );
    let up = make(
        3,
        Action::Vote {
            target: target.clone(),
            value: 1,
        },
    );
    let down = make(
        4,
        Action::Vote {
            target: target.clone(),
            value: -1,
        },
    );
    let events = vec![post.clone(), reply.clone(), up.clone(), down.clone()];
    let mut reversed = events.clone();
    reversed.reverse();
    let viewer = hex::encode(author.verifying_key().as_bytes());
    let projected = project(&events, "test", &viewer, &BTreeSet::new());
    assert_eq!(
        projected,
        project(&reversed, "test", &viewer, &BTreeSet::new())
    );
    assert_eq!(projected[0].score, -1);
    assert_eq!(projected[0].replies.len(), 1);
    let impostor = signing_key(&new_secret()).unwrap();
    let illegal = Envelope::accept(
        SignedEvent::create(
            &impostor,
            id.clone(),
            "test".into(),
            1,
            Action::Delete {
                target: target.clone(),
            },
        )
        .unwrap(),
        &home,
    )
    .unwrap();
    let mut invalid_owner = events.clone();
    invalid_owner.push(illegal);
    assert_eq!(
        project(&invalid_owner, "test", &viewer, &BTreeSet::new()).len(),
        1
    );
    reversed.insert(0, make(5, Action::Delete { target }));
    assert!(project(&reversed, "test", &viewer, &BTreeSet::new()).is_empty());
}
#[test]
fn closed_event_schema_rejects_unknown_fields() {
    let (_, author, id) = setup();
    let event = SignedEvent::create(&author, id, "test".into(), 1, post()).unwrap();
    let mut value = serde_json::to_value(event).unwrap();
    value["payload"]["futureField"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SignedEvent>(value).is_err());
}
#[test]
fn ffi_null_and_closed_handles_return_owned_errors_without_unwinding() {
    use std::ffi::CStr;
    use yappers_netkit::ffi::*;
    unsafe {
        let result = yappers_netkit_open(std::ptr::null());
        let json: serde_json::Value =
            serde_json::from_str(CStr::from_ptr(result).to_str().unwrap()).unwrap();
        yappers_netkit_string_free(result);
        assert_eq!(json["ok"], false);
        let command = std::ffi::CString::new(r#"{"type":"feed"}"#).unwrap();
        let result = yappers_netkit_request(0, command.as_ptr());
        assert!(
            CStr::from_ptr(result)
                .to_str()
                .unwrap()
                .contains("invalid or closed handle")
        );
        yappers_netkit_string_free(result);
        let close = yappers_netkit_close(0);
        yappers_netkit_string_free(close);
        yappers_netkit_string_free(std::ptr::null_mut());
    }
}

#[test]
fn rich_posts_bind_profile_and_headline_and_keep_legacy_encoding() {
    let (home, author, id) = setup();
    assert_eq!(
        serde_json::to_string(&post()).unwrap(),
        r#"{"kind":"post","body":"Hello neighbors","topic":"Friends","anonymous":true,"displayName":"Anonymous neighbor"}"#
    );
    let legacy = SignedEvent::create(&author, id.clone(), "test".into(), 1, post()).unwrap();
    assert_eq!(legacy.payload.version, 4);
    let mut action = Action::PostV2 {
        area: None,
        body: "Bring a book".into(),
        headline: "Quiet company".into(),
        topic: "Friends".into(),
        anonymous: false,
        display_name: "Tender comet".into(),
        profile: Some(Box::new(CommunityProfile {
            pronouns: "they / them".into(),
            bio: "Finding my people".into(),
            here_for: vec!["Chosen family".into()],
            ..Default::default()
        })),
    };
    let event = SignedEvent::create(&author, id.clone(), "test".into(), 2, action.clone()).unwrap();
    assert_eq!(event.payload.version, 4);
    let envelope = Envelope::accept(event.clone(), &home).unwrap();
    let roundtrip: Envelope =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    roundtrip.validate().unwrap();
    let view = project(&[roundtrip], "test", "", &BTreeSet::new());
    assert_eq!(view[0].headline, "Quiet company");
    assert_eq!(view[0].profile.as_ref().unwrap().pronouns, "they / them");
    let mut altered = event.clone();
    if let Action::PostV2 { profile, .. } = &mut altered.payload.action {
        profile.as_mut().unwrap().bio = "Tampered".into();
    }
    assert!(altered.validate().is_err());
    if let Action::PostV2 { headline, .. } = &mut action {
        *headline = "🦊".repeat(41);
    }
    assert!(SignedEvent::create(&author, id.clone(), "test".into(), 3, action.clone()).is_err());
    if let Action::PostV2 {
        headline,
        anonymous,
        display_name,
        ..
    } = &mut action
    {
        *headline = "Hello".into();
        *anonymous = true;
        *display_name = "Anonymous neighbor".into();
    }
    assert!(
        SignedEvent::create(&author, id.clone(), "test".into(), 4, action.clone()).is_err(),
        "anonymous profile leak rejected"
    );
    if let Action::PostV2 { profile, .. } = &mut action {
        *profile = None;
    }
    SignedEvent::create(&author, id, "test".into(), 5, action).unwrap();
    legacy.validate().unwrap();
}

#[test]
fn busy_conversations_keep_new_comments_in_the_recent_window() {
    let (home, author, id) = setup();
    let parent = Envelope::accept(
        SignedEvent::create(&author, id.clone(), "test".into(), 1, post()).unwrap(),
        &home,
    )
    .unwrap();
    let mut events = vec![parent.clone()];
    for n in 0..205 {
        let event = SignedEvent::create(
            &author,
            id.clone(),
            "test".into(),
            n + 2,
            Action::Reply {
                parent: parent.event.id.clone(),
                body: format!("Comment {n}"),
                display_name: "Neighbor".into(),
            },
        )
        .unwrap();
        let mut reply = Envelope::accept(event, &home).unwrap();
        // Projection-only fixture: unique acceptance times isolate the bounded-window policy.
        // Envelope signature verification is exercised independently above.
        reply.accepted_at = parent.accepted_at + n as i64 + 1;
        events.push(reply);
    }
    let projected = project(&events, "test", "", &BTreeSet::new());
    assert_eq!(projected[0].replies.len(), 200);
    assert_eq!(projected[0].replies.first().unwrap().body, "Comment 5");
    assert_eq!(projected[0].replies.last().unwrap().body, "Comment 204");
    events.reverse();
    assert_eq!(projected, project(&events, "test", "", &BTreeSet::new()));
}
