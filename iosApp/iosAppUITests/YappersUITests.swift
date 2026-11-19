import XCTest
import CoreLocation

final class YappersUITests: XCTestCase {
    override func setUp() { super.setUp(); continueAfterFailure = false }
    private func swipeContent(in app: XCUIApplication) {
        let top = app.keyboards.firstMatch.exists ? app.keyboards.firstMatch.frame.minY : app.frame.maxY
        let startY = min(app.frame.height * 0.78, top - 24)
        let origin = app.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
        origin.withOffset(CGVector(dx: app.frame.width * 0.5, dy: startY))
            .press(forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: app.frame.width * 0.5, dy: max(190, startY - 320))))
    }
    private func enter(_ text: String, into field: XCUIElement, app: XCUIApplication) {
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        if !app.keyboards.firstMatch.waitForExistence(timeout: 5) { field.tap() }
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        app.typeText(text)
    }
    private func writePost(_ body: String, app: XCUIApplication) {
        enter("A little hello", into: app.textViews["post-headline-input"], app: app)
        enter(body, into: app.textViews["post-body-input"], app: app)
    }
    func testWelcomeLayoutAndFooterLinks() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--welcome-ui-test", "--netkit-reset"]
        app.launch()
        XCTAssertTrue(app.staticTexts["Welcome to Yap"].waitForExistence(timeout: 20))
        let wordmark = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "yap")).firstMatch
        XCTAssertTrue(wordmark.exists)
        XCTAssertEqual(wordmark.frame.midX, app.frame.midX, accuracy: 3)
        XCTAssertGreaterThan(wordmark.frame.width, 100)
        let create = app.buttons["Create an account"]
        let restore = app.buttons["Restore an existing account"]
        XCTAssertTrue(create.isHittable && restore.isHittable)
        XCTAssertGreaterThanOrEqual(create.frame.height, 44)
        XCTAssertGreaterThanOrEqual(restore.frame.height, 44)
        XCTAssertLessThanOrEqual(create.frame.maxY, restore.frame.minY)
        let terms = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Terms of Service")).firstMatch
        let privacy = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Privacy Policy")).firstMatch
        XCTAssertTrue(terms.isHittable && privacy.isHittable)
        XCTAssertEqual(terms.frame.midY, privacy.frame.midY, accuracy: 4)
        XCTAssertLessThanOrEqual(terms.frame.maxX, privacy.frame.minX)
        XCTAssertGreaterThan(terms.frame.minY, restore.frame.maxY)
        XCTAssertGreaterThan(terms.frame.midY, app.frame.height * 0.82)
        for label in ["Pause motion", "Play motion"] {
            XCTAssertFalse(app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", label)).firstMatch.exists)
        }
        capture("Welcome footer", app: app)
        let browser = XCUIApplication(bundleIdentifier: "com.apple.mobilesafari")
        for link in [terms, privacy] {
            link.tap()
            XCTAssertTrue(browser.wait(for: .runningForeground, timeout: 12))
            app.activate()
            XCTAssertTrue(app.staticTexts["Welcome to Yap"].waitForExistence(timeout: 8))
        }
        create.tap()
        XCTAssertTrue(app.staticTexts["Create your profile"].waitForExistence(timeout: 5))
        capture("New profile", app: app)
        app.buttons["Back"].tap()
        restore.tap()
        XCTAssertTrue(app.staticTexts["Welcome back"].waitForExistence(timeout: 5))
    }
    func testSignupProfileDraftAndDiscard() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--welcome-ui-test", "--netkit-reset"]
        app.launch()
        app.buttons["Create an account"].tap()
        let next = app.buttons["Continue to username"]
        XCTAssertTrue(next.waitForExistence(timeout: 10) && next.isEnabled)
        capture("Signup profile appearance", app: app)
        let hereFor = app.buttons["Here for"]
        reveal(hereFor, in: app)
        hereFor.tap()
        app.cells["Friends"].tap()
        app.navigationBars.buttons["Done"].tap()
        app.buttons["Back"].tap()
        app.alerts.buttons["Keep editing"].tap()
        hereFor.tap()
        XCTAssertTrue(app.cells["Friends"].isSelected)
        app.navigationBars.buttons["Done"].tap()
        app.buttons["Back"].tap()
        app.alerts.buttons["Discard"].tap()
        app.buttons["Create an account"].tap()
        reveal(hereFor, in: app)
        hereFor.tap()
        XCTAssertFalse(app.cells["Friends"].isSelected)
    }

    func testRestoreMethodSelection() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--welcome-ui-test", "--netkit-reset"]
        app.launch()
        app.buttons["Restore an existing account"].tap()
        let manual = app.buttons["Enter a recovery key"]
        XCTAssertTrue(manual.waitForExistence(timeout: 8) && manual.isHittable)
        XCTAssertFalse(app.secureTextFields.firstMatch.exists)
        capture("Restore options", app: app)
        app.buttons["Use a key from Keychain"].tap()
        XCTAssertTrue(app.staticTexts["No Yap recovery keys in this Keychain yet."].waitForExistence(timeout: 5))
        manual.tap()
        let keyBar = app.navigationBars["Recovery key"]
        let confirm = keyBar.buttons["Restore"]
        XCTAssertTrue(confirm.waitForExistence(timeout: 5))
        XCTAssertFalse(confirm.isEnabled)
        capture("Recovery key entry", app: app)
        enter("example", into: app.secureTextFields["Recovery key"], app: app)
        XCTAssertTrue(confirm.isEnabled)
        keyBar.buttons["Cancel"].tap()
        app.buttons["Use an encrypted app backup"].tap()
        let fileBar = app.navigationBars["Encrypted backup"]
        XCTAssertTrue(fileBar.waitForExistence(timeout: 5))
        XCTAssertTrue(app.cells["Choose backup file"].isHittable)
        XCTAssertTrue(app.secureTextFields["Backup passphrase"].exists)
        XCTAssertFalse(fileBar.buttons["Restore"].isEnabled)
        capture("Backup file restore", app: app)
        fileBar.buttons["Cancel"].tap()
        manual.tap()
        XCTAssertFalse(confirm.isEnabled)
        keyBar.buttons["Cancel"].tap()
        app.buttons["Back"].tap()
        XCTAssertTrue(app.buttons["Create an account"].waitForExistence(timeout: 5))
    }

    func testFirstVisitAndReturningAccount() throws {
        let fixture = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "NetkitTestJoin", withExtension: "json"))
        let data = try Data(contentsOf: fixture)
        guard (try JSONSerialization.jsonObject(with: data) as? [String: Any])?["version"] as? Int == 1 else {
            throw XCTSkip("Requires the isolated public enrollment fixture")
        }
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--welcome-ui-test", "--netkit-reset"]
        app.launchEnvironment["YAP_TEST_ENROLLMENT"] = String(decoding: data, as: UTF8.self)
        app.launch()
        XCTAssertTrue(app.staticTexts["Welcome to Yap"].waitForExistence(timeout: 20))
        app.buttons["Restore an existing account"].tap()
        XCTAssertTrue(app.staticTexts["Welcome back"].waitForExistence(timeout: 5))
        app.buttons["Back"].tap()
        app.buttons["Create an account"].tap()
        XCTAssertTrue(app.staticTexts["Create your profile"].waitForExistence(timeout: 8))
        capture("New profile", app: app)
        app.buttons["Name and pronouns"].tap()
        app.cells["Suggest another name"].tap()
        let displayName = try XCTUnwrap(app.textFields["Display name"].value as? String)
        enter("they / them", into: app.textFields["Pronouns"], app: app)
        app.navigationBars.buttons["Done"].tap()
        app.buttons["About you"].tap()
        enter("Growing lavender", into: app.textViews["Bio"], app: app)
        app.buttons["Dismiss keyboard"].tap()
        enter("Queer gardener", into: app.textFields["Identity, in your words"], app: app)
        app.buttons["Dismiss keyboard"].tap()
        let joys = app.textViews["Current joys"]
        reveal(joys, in: app)
        enter("Seed swaps", into: joys, app: app)
        app.navigationBars.buttons["Done"].tap()
        app.buttons["Here for"].tap()
        app.cells["Friends"].tap()
        app.navigationBars.buttons["Done"].tap()
        app.buttons["Add photo"].tap()
        let photo = app.images.matching(NSPredicate(format: "label BEGINSWITH 'Photo,'")).firstMatch
        XCTAssertTrue(photo.waitForExistence(timeout: 90), "Photos should load the seeded image on a fresh simulator")
        let frame = photo.frame
        app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: frame.midX, dy: frame.midY)).tap()
        let crop = app.buttons["Use crop"]
        XCTAssertTrue(crop.waitForExistence(timeout: 12))
        crop.tap()
        XCTAssertTrue(app.buttons["Change photo"].waitForExistence(timeout: 10))
        capture("Profile before enrollment", app: app)
        app.buttons["Continue to username"].tap()
        let username = app.textFields["onboarding-username"]
        XCTAssertTrue(username.waitForExistence(timeout: 35))
        capture("Signup username", app: app)
        XCTAssertFalse(app.buttons["Claim username"].isEnabled)
        XCTAssertFalse(app.buttons["Enter neighborhood"].exists)
        XCTAssertFalse(app.tabBars.buttons["Nearby"].exists)
        app.terminate()
        app.launchArguments.removeAll { $0 == "--netkit-reset" }
        app.launch()
        XCTAssertTrue(username.waitForExistence(timeout: 30), "An interrupted enrollment must resume at username")
        XCTAssertFalse(app.buttons["Create an account"].exists)
        enter("Hazel_test", into: username, app: app)
        XCTAssertEqual(username.value as? String, "hazel_test")
        app.buttons["Dismiss keyboard"].tap()
        let claim = app.buttons["Claim username"]
        reveal(claim, in: app)
        claim.tap()
        XCTAssertTrue(app.staticTexts["Keep your account"].waitForExistence(timeout: 35))
        app.buttons["Get recovery key"].tap()
        let save = app.cells["Save to Keychain"]
        XCTAssertTrue(save.waitForExistence(timeout: 10))
        save.tap()
        XCTAssertTrue(app.staticTexts["Saved to Keychain"].waitForExistence(timeout: 8))
        app.buttons["Done"].tap()
        app.buttons["Enter neighborhood"].tap()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.buttons["Sync now"].exists)
        app.swipeDown()
        app.buttons["Inbox"].tap()
        XCTAssertTrue(app.staticTexts["No conversations yet"].waitForExistence(timeout: 10))
        capture("Empty inbox", app: app)
        app.cells["Browse posts"].tap()
        XCTAssertTrue(app.buttons["All cities"].exists)
        app.terminate()
        app.launchArguments = ["--netkit-ui-test", "--welcome-ui-test"]
        app.launch()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 20))
        XCTAssertFalse(app.staticTexts["Welcome to Yap"].exists)
        app.buttons["You"].tap()
        for detail in [displayName, "they / them", "Growing lavender", "Queer gardener", "Seed swaps", "Friends"] {
            XCTAssertTrue(app.staticTexts[detail].waitForExistence(timeout: 5), "Missing signup profile detail: " + detail)
        }
        XCTAssertTrue(app.images["Profile photo of " + displayName].exists)
        capture("Saved signup profile", app: app)
        app.tabBars.buttons["Nearby"].tap()
        app.buttons["Post"].tap()
        let draft = "Testing my posting identity."
        writePost(draft, app: app)
        app.buttons["Dismiss keyboard"].tap()
        app.buttons["Post as Anonymous"].tap()
        app.descendants(matching: .any)["named-posting-identity"].firstMatch.tap()
        XCTAssertTrue(app.buttons["Post as hazel_test@neighborhood.test"].waitForExistence(timeout: 20))
        XCTAssertEqual(app.textViews["post-body-input"].value as? String, draft)
        app.buttons["Post to community"].tap()
        let namedPost = app.buttons.matching(NSPredicate(format: "label CONTAINS 'Testing my posting identity.'")).firstMatch
        XCTAssertTrue(namedPost.waitForExistence(timeout: 15))
        namedPost.tap()
        app.textViews["reply-body"].tap()
        XCTAssertTrue(app.buttons["Reply as hazel_test@neighborhood.test"].waitForExistence(timeout: 5))
        app.buttons["Reply as hazel_test@neighborhood.test"].tap()
        app.descendants(matching: .any)["Anonymous"].firstMatch.tap()
        XCTAssertTrue(app.buttons["Reply as Anonymous"].exists)
        app.terminate()
        app.launchArguments.append("--simulate-reinstall")
        app.launch()
        XCTAssertTrue(app.staticTexts["Welcome to Yap"].waitForExistence(timeout: 15))
        app.buttons["Continue with my account"].tap()
        XCTAssertTrue(app.staticTexts["What should neighbors call you?"].waitForExistence(timeout: 5))
        capture("Returning account name", app: app)
        app.buttons["Suggest another name"].tap()
        app.buttons["Continue"].tap()
        XCTAssertTrue(app.staticTexts["Keep your account"].waitForExistence(timeout: 15))
        app.buttons["Enter neighborhood"].tap()
        app.terminate()
        app.launchArguments.removeLast()
        app.launch()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 15))
        app.buttons["You"].tap()
        XCTAssertTrue(app.staticTexts["hazel_test@neighborhood.test"].waitForExistence(timeout: 40))

        app.terminate()
        app.launchArguments.append("--review-device-reset")
        app.launch()
        XCTAssertTrue(app.staticTexts["Start over on this iPhone?"].waitForExistence(timeout: 10))
        app.buttons["Keep my account"].tap()
        app.buttons["You"].tap()
        XCTAssertTrue(app.staticTexts["hazel_test@neighborhood.test"].waitForExistence(timeout: 20))
        app.terminate()
        app.launch()
        app.buttons["Erase Yap data on this iPhone"].tap()
        XCTAssertTrue(app.buttons["Create an account"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.buttons["Continue with my account"].exists)
        app.terminate()
        app.launchArguments.removeLast()
        app.launch()
        XCTAssertTrue(app.buttons["Create an account"].waitForExistence(timeout: 15))
        app.buttons["Restore an existing account"].tap()
        app.buttons["Use a key from Keychain"].tap()
        let restore = app.navigationBars["Recovery key"].buttons["Restore"]
        XCTAssertTrue(restore.waitForExistence(timeout: 5) && restore.isEnabled)
        restore.tap()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 40))
        app.buttons["You"].tap()
        XCTAssertTrue(app.staticTexts["hazel_test@neighborhood.test"].waitForExistence(timeout: 30))
        let guide = app.buttons["Welcome guide"]
        let guideBar = app.navigationBars["Welcome guide"]
        reveal(guide, in: app)
        guide.tap()
        XCTAssertTrue(guideBar.waitForExistence(timeout: 5))
        guideBar.buttons.firstMatch.tap()
        XCTAssertTrue(app.tabBars.buttons["You"].isSelected)
        guide.tap()
        app.buttons["Return to neighborhood"].tap()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.tabBars.buttons["Nearby"].isSelected)
    }

    func testFeedComposerAndSaved() {
        let message = "Hello neighbors " + UUID().uuidString.prefix(8) + ". Anyone up for a park walk?"
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 20))
        let feed = XCTAttachment(screenshot: app.screenshot())
        feed.name = "Nearby feed"
        feed.lifetime = .keepAlways
        add(feed)
        let tabs = app.tabBars.firstMatch
        app.navigationBars.buttons["Inbox"].tap()
        XCTAssertTrue(app.navigationBars["Inbox"].waitForExistence(timeout: 5))
        tabs.buttons["Saved"].tap()
        XCTAssertTrue(app.staticTexts["Saved posts"].waitForExistence(timeout: 5))
        app.navigationBars.buttons["Post"].tap()
        XCTAssertTrue(app.staticTexts["New post"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["Post to nearby"].isEnabled)
        writePost(message, app: app)
        XCTAssertTrue(app.navigationBars.buttons["Post to nearby"].isHittable)
        let typing = XCTAttachment(screenshot: app.screenshot())
        typing.name = "Composer with keyboard"
        typing.lifetime = .keepAlways
        add(typing)
        app.buttons["Dismiss keyboard"].tap()
        let toolbar = app.navigationBars["New post"]
        let toolbarFrame = toolbar.frame
        let topic = app.buttons["Topic"]
        reveal(topic, in: app)
        topic.tap()
        app.buttons["Missed connections"].tap()
        XCTAssertEqual(topic.value as? String, "Missed connections")
        XCTAssertEqual(toolbar.frame.minY, toolbarFrame.minY, accuracy: 1)
        app.buttons["Post to nearby"].tap()
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", message)).firstMatch.waitForExistence(timeout: 5))
        XCTAssertTrue(tabs.buttons["Nearby"].isSelected)
        XCTAssertTrue(app.buttons["All cities"].exists)
        app.buttons.matching(NSPredicate(format: "label CONTAINS %@", message)).firstMatch.tap()
        XCTAssertTrue(app.staticTexts["Comments"].waitForExistence(timeout: 5))
        let reply = "A slow walk sounds perfect."
        enter(reply, into: app.textViews["reply-body"], app: app)
        app.buttons["Post comment"].tap()
        XCTAssertTrue(app.staticTexts[reply].waitForExistence(timeout: 5))
        let conversation = XCTAttachment(screenshot: app.screenshot())
        conversation.name = "Conversation after reply"
        conversation.lifetime = .keepAlways
        add(conversation)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.buttons["Save post"].firstMatch.tap()
        app.buttons.matching(NSPredicate(format: "label CONTAINS 'Saved'")).firstMatch.tap()
        XCTAssertTrue(app.staticTexts["Saved posts"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", message)).firstMatch.exists)
        app.terminate()
        app.launchArguments = ["--netkit-ui-test"]
        app.launch()
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", message)).firstMatch.waitForExistence(timeout: 15))
    }
    func testFeedFiltersAndLocalSafetyActions() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        let filter = app.buttons["Filter"]
        XCTAssertTrue(filter.waitForExistence(timeout: 15))
        filter.tap()
        XCTAssertTrue(app.navigationBars["Feed filters"].waitForExistence(timeout: 5))
        let sort = app.segmentedControls.firstMatch
        sort.buttons["Popular"].tap()
        XCTAssertTrue(sort.buttons["Popular"].isSelected)
        sort.buttons["New"].tap()
        app.cells["Plans"].tap()
        capture("Native feed sort and topics", app: app)
        app.buttons["Apply"].tap()
        let filteredPost = app.buttons.matching(NSPredicate(format: "label CONTAINS 'Soup & unserious opinions'")).firstMatch
        XCTAssertTrue(filteredPost.waitForExistence(timeout: 5))
        app.buttons["Filter (1)"].tap()
        app.cells["All"].tap()
        app.buttons["Apply"].tap()
        let up = app.buttons["Upvote"].firstMatch
        let down = app.buttons["Downvote"].firstMatch
        XCTAssertTrue(app.staticTexts["28"].isHittable)
        up.tap()
        XCTAssertTrue(up.isSelected)
        XCTAssertTrue(app.staticTexts["29"].isHittable)
        down.tap()
        XCTAssertFalse(up.isSelected)
        XCTAssertTrue(down.isSelected)
        XCTAssertTrue(app.staticTexts["27"].isHittable)
        capture("Feed controls after changing vote", app: app)
        down.tap()
        XCTAssertTrue(app.staticTexts["28"].isHittable)
        app.buttons["2 replies"].tap()
        let replyOptions = app.buttons["Reply options for Moss enthusiast"]
        XCTAssertTrue(replyOptions.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["Block account"].exists)
        replyOptions.tap()
        app.descendants(matching: .any)["Block account"].firstMatch.tap()
        app.buttons["Cancel"].tap()
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.buttons["Post options"].firstMatch.tap()
        app.descendants(matching: .any)["Hide on this device"].firstMatch.tap()
        app.buttons["Hide post"].tap()
        let hidden = app.buttons.matching(NSPredicate(format: "label CONTAINS 'A book club for the quietly social'")).firstMatch
        XCTAssertTrue(hidden.waitForNonExistence(timeout: 5))
        app.terminate()
        app.launchArguments = ["--netkit-ui-test"]
        app.launch()
        XCTAssertTrue(filter.waitForExistence(timeout: 15))
        XCTAssertFalse(hidden.exists)
    }

    func testNativeAccountBackupAndMove() throws {
        let file = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "NetkitTestJoin", withExtension: "json"))
        let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
        guard let source = fixture["source"], let destination = fixture["destination"] else {
            throw XCTSkip("Run NETKIT_TEST_SCOPE=recovery scripts/test-netkit-ios.sh for the two-home recovery fixture.")
        }
        let sourceJoin = String(decoding: try JSONSerialization.data(withJSONObject: source), as: UTF8.self)
        let destinationJoin = String(decoding: try JSONSerialization.data(withJSONObject: destination), as: UTF8.self)
        let postBody = "A story to keep when moving homes."
        let password = "Neighborhood walk 26"
        let backupName = "Yap-QA-" + UUID().uuidString.prefix(8)
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        let you = app.tabBars.buttons["You"]
        XCTAssertTrue(you.waitForExistence(timeout: 20))
        you.tap()
        let join = app.cells["Join community"]
        reveal(join, in: app)
        join.tap()
        let joinButton = app.navigationBars.buttons["Join instance"]
        XCTAssertFalse(joinButton.isEnabled)
        enter(sourceJoin, into: app.secureTextFields["Private community invitation"], app: app)
        joinButton.tap()
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))

        app.navigationBars.buttons["Post"].tap()
        writePost(postBody, app: app)
        app.buttons["Post to community"].tap()
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", postBody)).firstMatch.waitForExistence(timeout: 15))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
        you.tap()
        let files = app.cells["Backup files"]
        reveal(files, in: app)
        files.tap()
        let export = app.cells["Save a backup file"]
        reveal(export, in: app)
        export.tap()
        let save = app.navigationBars.buttons["Save backup"]
        XCTAssertFalse(save.isEnabled)
        capture("Native encrypted backup form", app: app)
        enter(password, into: app.secureTextFields["Backup password"], app: app)
        XCTAssertFalse(save.isEnabled)
        enter(password, into: app.secureTextFields["Confirm password"], app: app)
        XCTAssertTrue(save.isEnabled)
        save.tap()
        let saveFile = app.buttons["Save"]
        let onDevice = app.cells["DOC.sidebar.item.On My iPhone"]
        let pickerReady = expectation(for: NSPredicate { _, _ in saveFile.exists || onDevice.exists }, evaluatedWith: app)
        wait(for: [pickerReady], timeout: 90)
        if onDevice.exists && onDevice.isHittable { onDevice.tap() }
        XCTAssertTrue(saveFile.waitForExistence(timeout: 15), "Export should open a writable Files location")
        let fileName = app.textFields.matching(NSPredicate(format: "value BEGINSWITH 'Yap-Recovery'")).firstMatch
        XCTAssertTrue(fileName.waitForExistence(timeout: 5))
        fileName.tap()
        fileName.typeKey("a", modifierFlags: .command)
        fileName.typeText(backupName)
        saveFile.tap()
        let notice = app.staticTexts["Backup file saved."]
        XCTAssertTrue(notice.waitForExistence(timeout: 15))

        let move = app.cells["Move to another community"]
        reveal(move, in: app)
        move.tap()
        let moveAction = app.navigationBars.buttons["Move account"]
        XCTAssertFalse(moveAction.isEnabled)
        app.cells["Choose backup file"].tap()
        let backup = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", backupName)).firstMatch
        if !backup.waitForExistence(timeout: 5) {
            let browse = app.buttons["Browse"].firstMatch
            if !onDevice.exists && browse.waitForExistence(timeout: 15) { browse.tap() }
            if onDevice.waitForExistence(timeout: 3) && onDevice.isHittable { onDevice.tap() }
        }
        XCTAssertTrue(backup.waitForExistence(timeout: 12), "Files should list the exported age backup")
        backup.tap()
        XCTAssertTrue(app.staticTexts["Encrypted backup selected"].waitForExistence(timeout: 10))
        enter(password, into: app.secureTextFields["Backup password"], app: app)
        app.buttons["Dismiss keyboard"].tap()
        let destinationField = app.secureTextFields["New community invitation"]
        destinationField.tap()
        destinationField.typeText(destinationJoin)
        let canMove = expectation(for: NSPredicate(format: "enabled == true"), evaluatedWith: moveAction)
        wait(for: [canMove], timeout: 8)
        moveAction.tap()
        XCTAssertTrue(app.staticTexts["Account moved. You can now rejoin your groups."].waitForExistence(timeout: 40))
        let history = app.cells["Backed-up posts and replies"]
        reveal(history, in: app)
        history.tap()
        let retained = app.textViews.matching(NSPredicate(format: "value == %@", postBody)).firstMatch
        XCTAssertTrue(retained.waitForExistence(timeout: 10), "The new home retains the published post in selectable history")
        capture("Native backed-up history after changing home", app: app)
        app.navigationBars.buttons["Done"].tap()
    }

    func testBlockAndUnblockFromInbox() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        let post = app.buttons.matching(NSPredicate(format: "label CONTAINS 'A book club for the quietly social'")).firstMatch
        XCTAssertTrue(post.waitForExistence(timeout: 20))
        app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "View Soft landing's profile")).firstMatch.tap()
        reveal(app.cells["Block account"], in: app)
        app.cells["Block account"].tap()
        XCTAssertTrue(app.alerts.buttons["Cancel"].waitForExistence(timeout: 5), app.debugDescription)
        app.alerts.buttons["Block account"].tap()
        let hidden = expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: post)
        wait(for: [hidden], timeout: 6)
        app.buttons["Inbox"].tap()
        app.buttons["Inbox settings"].tap()
        app.buttons["Blocked accounts"].tap()
        XCTAssertTrue(app.staticTexts["Soft landing"].waitForExistence(timeout: 5))
        app.cells["Unblock"].tap()
        XCTAssertTrue(app.staticTexts["You haven’t blocked anyone."].waitForExistence(timeout: 5))
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(post.waitForExistence(timeout: 5))
    }

    func testEncryptedConversation() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "NetkitTestJoin", withExtension: "json"))
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        guard object["networkMode"] as? String == "local" else { throw XCTSkip("Run the local messaging fixture") }
        let invitation = String(decoding: try JSONSerialization.data(withJSONObject: object), as: UTF8.self)
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 20))
        app.buttons["You"].tap()
        reveal(app.cells["Join community"], in: app)
        app.cells["Join community"].tap()
        let field = app.secureTextFields.firstMatch.exists ? app.secureTextFields.firstMatch : app.textViews.firstMatch
        enter(invitation, into: field, app: app)
        app.buttons["Join instance"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
        app.buttons["Inbox"].tap()
        app.buttons["Inbox settings"].tap()
        let privacy = app.buttons["Privacy"]
        reveal(privacy, in: app); privacy.tap()
        let approved = app.switches.matching(NSPredicate(format: "label CONTAINS 'Approved people only'")).firstMatch
        reveal(approved, in: app); approved.tap()
        capture("Message privacy", app: app)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.tabBars.buttons["Nearby"].tap()
        let profile = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "View DM test neighbor's profile")).firstMatch
        XCTAssertTrue(profile.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["Message"].exists, "Start direct messages from the author's profile")
        capture("Feed without a message shortcut", app: app)
        profile.tap()
        reveal(app.cells["Message"], in: app)
        capture("Message from the author's profile", app: app)
        app.cells["Message"].tap()
        XCTAssertTrue(app.staticTexts["End-to-end encrypted"].waitForExistence(timeout: 8))
        let editor = app.textViews["direct-message-body"]
        let send = app.buttons["Send"]
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        editor.tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        XCTAssertTrue(send.waitForExistence(timeout: 5))
        XCTAssertFalse(send.isEnabled)
        app.typeText("Hello from iOS")
        XCTAssertEqual(editor.value as? String, "Hello from iOS")
        XCTAssertTrue(send.isEnabled && send.isHittable)
        capture("Native message editor with keyboard", app: app)
        send.tap()
        XCUIDevice.shared.press(.home)
        let leftApp = expectation(for: NSPredicate { _, _ in
            app.state == .runningBackground || app.state == .runningBackgroundSuspended
        }, evaluatedWith: nil)
        wait(for: [leftApp], timeout: 10)
        app.activate()
        let reply = app.staticTexts["Received your encrypted message"]
        XCTAssertTrue(reply.waitForExistence(timeout: 50))
        app.terminate()
        app.launchArguments = ["--netkit-ui-test"]
        app.launch()
        app.buttons["Inbox"].tap()
        let thread = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", "Received your encrypted message", "Received your encrypted message")).firstMatch
        XCTAssertTrue(thread.waitForExistence(timeout: 10))
        let requests = app.cells["Message requests (1)"]
        XCTAssertTrue(requests.waitForExistence(timeout: 30))
        XCTAssertFalse(app.staticTexts["Hello after approval"].exists)
        requests.tap()
        XCTAssertTrue(app.cells["Accept"].waitForExistence(timeout: 10))
        capture("Incoming request before acceptance", app: app)
        app.cells["Accept"].tap()
        app.navigationBars.buttons.element(boundBy: 0).tap()
        let accepted = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS 'Hello after approval' OR value CONTAINS 'Hello after approval'")).firstMatch
        XCTAssertTrue(accepted.waitForExistence(timeout: 30))
        thread.tap()
        XCTAssertTrue(app.staticTexts["Hello from iOS"].waitForExistence(timeout: 8))
        let report = app.buttons["Report message"].firstMatch
        reveal(report, in: app)
        report.tap()
        let reason = app.buttons["Reason"]
        XCTAssertTrue(reason.waitForExistence(timeout: 8))
        reason.tap()
        app.buttons["Harassment"].tap()
        let excerpt = app.textViews["Evidence for the moderator"]
        XCTAssertTrue(excerpt.exists)
        XCTAssertFalse(app.cells["Review report"].isEnabled)
        app.cells["Use this message’s text"].tap()
        XCTAssertEqual(excerpt.value as? String, "Received your encrypted message")
        let review = app.cells["Review report"]
        reveal(review, in: app)
        review.tap()
        let receipt = app.textViews["Private receipt"]
        XCTAssertTrue(receipt.waitForExistence(timeout: 15))
        let code = try XCTUnwrap(receipt.value as? String)
        XCTAssertFalse(code.isEmpty)
        let submit = app.cells["Send these details"]
        reveal(submit, in: app)
        capture("Reviewing selected message evidence", app: app)
        submit.tap()
        XCTAssertTrue(app.staticTexts["Received. Awaiting review."].waitForExistence(timeout: 15))
        app.buttons["Done"].tap()
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.buttons["Inbox settings"].tap()
        app.buttons["Privacy"].tap()
        app.cells["Check a private receipt"].tap()
        let receiptInput = app.textFields["Private receipt"]
        enter(code, into: receiptInput, app: app)
        app.buttons["Dismiss keyboard"].tap()
        app.cells["Check status"].tap()
        XCTAssertTrue(app.staticTexts["Received. Awaiting review."].waitForExistence(timeout: 15))
    }

    func testFederatedNetkit() throws {
        let bundle = Bundle(for: Self.self)
        let url = try XCTUnwrap(bundle.url(forResource: "NetkitTestJoin", withExtension: "json"))
        let object = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
        if object["skip"] as? Bool == true { throw XCTSkip("Run scripts/test-netkit-ios.sh for the live federation fixture") }
        let invitation = String(data: try JSONSerialization.data(withJSONObject: object), encoding: .utf8)!
        let message = "Federated iPhone smoke " + UUID().uuidString.prefix(8)
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 20))
        app.buttons["You"].tap()
        let profile = XCTAttachment(screenshot: app.screenshot())
        profile.name = "Netkit connection profile"; profile.lifetime = .keepAlways; add(profile)
        reveal(app.cells["Join community"], in: app); app.cells["Join community"].tap()
        XCTAssertTrue(app.staticTexts["Join your community"].waitForExistence(timeout: 5))
        let joinSheet = XCTAttachment(screenshot: app.screenshot())
        joinSheet.name = "Netkit private invitation sheet"; joinSheet.lifetime = .keepAlways; add(joinSheet)
        let field = app.secureTextFields.firstMatch.exists ? app.secureTextFields.firstMatch : app.textViews.firstMatch
        enter(invitation, into: field, app: app)
        app.buttons["Join instance"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
        app.buttons["Post"].tap()
        writePost(message, app: app)
        app.buttons["Dismiss keyboard"].tap()
        app.buttons["Post as Anonymous"].tap()
        app.descendants(matching: .any)["named-posting-identity"].firstMatch.tap()
        app.buttons["Post to community"].tap()
        let post = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", message)).firstMatch
        XCTAssertTrue(post.waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
        post.tap()
        enter("Reply from the Kotlin iPhone client.", into: app.textViews["reply-body"], app: app)
        app.buttons["Post comment"].tap()
        let liveComment = app.staticTexts["Reply from the Kotlin iPhone client."]
        XCTAssertTrue(liveComment.waitForExistence(timeout: 10))
        let visibleComment = expectation(for: NSPredicate { _, _ in
            !app.keyboards.firstMatch.exists && liveComment.exists && liveComment.isHittable
        }, evaluatedWith: app)
        wait(for: [visibleComment], timeout: 10)
        capture("Federated comment revealed", app: app)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.buttons["Upvote"].firstMatch.tap()
        app.buttons["Save post"].firstMatch.tap()
        app.buttons["Saved"].tap()
        XCTAssertTrue(post.waitForExistence(timeout: 10))
        let live = XCTAttachment(screenshot: app.screenshot())
        live.name = "Iroh live saved feed"; live.lifetime = .keepAlways; add(live)
        app.terminate(); app.launchArguments = ["--netkit-ui-test"]; app.launch()
        XCTAssertTrue(post.waitForExistence(timeout: 20))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
    }

    private func reveal(_ element: XCUIElement, in app: XCUIApplication) {
        for _ in 0..<7 {
            if element.exists && element.isHittable { break }
            swipeContent(in: app)
        }
        XCTAssertTrue(element.exists && element.isHittable, "Expected control is reachable")
        // Compose scrolling can still decelerate after XCTest reports the swipe idle.
        // A tap during that deceleration only stops scrolling; wait for a stable target.
        var previous = CGRect.zero
        var stableSamples = 0
        let stable = expectation(for: NSPredicate { _, _ in
            let frame = element.frame
            stableSamples = frame == previous && element.isHittable ? stableSamples + 1 : 0
            previous = frame
            return stableSamples >= 2
        }, evaluatedWith: element)
        wait(for: [stable], timeout: 6)
    }
    private func capture(_ name: String, app: XCUIApplication) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
    func testLocalGroupsAutoFederation() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "NetkitTestJoin", withExtension: "json"))
        let object = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
        if object["skip"] as? Bool == true { throw XCTSkip("Run scripts/test-netkit-ios.sh for the live fixture") }
        let invitation = String(data: try JSONSerialization.data(withJSONObject: object), encoding: .utf8)!
        let suffix = String(UUID().uuidString.prefix(8))
        let localMessage = "Local group iPhone " + suffix
        let globalMessage = "Worldwide group iPhone " + suffix
        let createdMessage = "Created group iPhone " + suffix
        let createdName = "iPhone circle " + suffix
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 20))
        app.tabBars.buttons["You"].tap()
        app.cells["invitation"].tap()
        let invitationField = app.secureTextFields.firstMatch.exists ? app.secureTextFields.firstMatch : app.textViews.firstMatch
        enter(invitation, into: invitationField, app: app)
        app.buttons["Join instance"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))

        app.tabBars.buttons["Groups"].tap()
        let scope = app.segmentedControls["scope"]
        XCTAssertTrue(scope.waitForExistence(timeout: 5))
        XCTAssertTrue(app.searchFields["search"].exists)
        scope.buttons["Local"].tap()
        app.cells["city"].tap()
        XCTAssertTrue(app.navigationBars["Choose a city"].waitForExistence(timeout: 5))
        enter("New York", into: app.searchFields["find"], app: app)
        app.descendants(matching: .any)["city-New York, NY, US"].firstMatch.tap()
        capture("Groups local catalog", app: app)
        let joinLocal = app.cells["Join group Neighborhood book club"]
        joinLocal.tap()
        XCTAssertTrue(app.buttons["Post"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
        app.buttons["Post"].tap()
        writePost(localMessage, app: app)
        app.buttons["Post to community"].tap()
        let localPost = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", localMessage)).firstMatch
        XCTAssertTrue(localPost.waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
        app.buttons["Save post"].firstMatch.tap()
        capture("Local group feed", app: app)

        app.tabBars.buttons["Groups"].tap()
        scope.buttons["Worldwide"].tap()
        capture("Groups worldwide catalog", app: app)
        let joinGlobal = app.cells["Join group Queer everywhere"]
        joinGlobal.tap()
        XCTAssertTrue(app.buttons["Post"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
        XCTAssertFalse(localPost.exists, "Local posts do not leak into the worldwide group")
        app.buttons["Post"].tap()
        writePost(globalMessage, app: app)
        app.buttons["Post to community"].tap()
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", globalMessage)).firstMatch.waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))

        app.tabBars.buttons["Groups"].tap()
        scope.buttons["Joined"].tap()
        let reopen = app.cells["Open group Neighborhood book club"]
        reopen.tap()
        XCTAssertTrue(localPost.waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["Unsave post"].firstMatch.exists)
        app.tabBars.buttons["Groups"].tap()
        app.cells["home"].tap()
        XCTAssertTrue(app.buttons["All cities"].waitForExistence(timeout: 10))
        app.tabBars.buttons["Groups"].tap()
        app.cells["create"].tap()
        let form = app.navigationBars["New group"]
        XCTAssertTrue(form.waitForExistence(timeout: 5))
        XCTAssertFalse(form.buttons["Create group"].isEnabled)
        enter("A discarded draft", into: app.textFields["name"], app: app)
        form.buttons["Cancel"].tap()
        app.alerts["Discard group draft?"].buttons["Discard draft"].tap()
        XCTAssertTrue(form.waitForNonExistence(timeout: 5))
        app.cells["create"].tap()
        enter(createdName, into: app.textFields["name"], app: app)
        form.buttons["Cancel"].tap()
        let discard = app.alerts["Discard group draft?"]
        XCTAssertTrue(discard.waitForExistence(timeout: 5))
        discard.buttons["Keep editing"].tap()
        XCTAssertEqual(app.textFields["name"].value as? String, createdName)
        let descriptionView = app.textViews["description"]
        enter("Meet people through this public local group.", into: descriptionView, app: app)
        capture("Group creation with keyboard", app: app)
        form.buttons["Create group"].tap()
        XCTAssertTrue(app.buttons["Post"].waitForExistence(timeout: 15))
        XCTAssertTrue(app.staticTexts[createdName].waitForExistence(timeout: 10))
        app.buttons["Post"].tap()
        writePost(createdMessage, app: app)
        app.buttons["Post to community"].tap()
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", createdMessage)).firstMatch.waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
        app.terminate(); app.launchArguments = ["--netkit-ui-test"]; app.launch()
        XCTAssertTrue(app.staticTexts[createdName].waitForExistence(timeout: 20))
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", createdMessage)).firstMatch.waitForExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["updated-community-feed"].firstMatch.waitForExistence(timeout: 30))
    }

    func testEditingDisplayNameKeepsEditorAndKeyboardInPlace() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        app.buttons["You"].tap()
        let edit = app.buttons["Edit profile"]
        let privacy = app.buttons["Privacy"]
        XCTAssertTrue(edit.waitForExistence(timeout: 10))
        XCTAssertEqual(edit.frame.midY, privacy.frame.midY, accuracy: 4)
        edit.tap()
        let field = app.descendants(matching: .any).matching(NSPredicate(
            format: "(elementType == %d OR elementType == %d) AND label CONTAINS 'Display name'",
            XCUIElement.ElementType.textField.rawValue, XCUIElement.ElementType.textView.rawValue)).firstMatch
        field.tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        let save = app.buttons["Save profile"]
        let originalY = save.frame.midY
        app.typeText(XCUIKeyboardKey.delete.rawValue)
        for character in ["X", "y"] {
            XCTAssertTrue(save.exists && save.isHittable)
            XCTAssertTrue(app.keyboards.firstMatch.exists)
            XCTAssertEqual(save.frame.midY, originalY, accuracy: 4)
            app.typeText(character)
        }
        XCTAssertTrue(save.isEnabled, app.debugDescription)
        save.tap()
        XCTAssertTrue(edit.waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS[c] 'xy'")).firstMatch.waitForExistence(timeout: 5), app.debugDescription)
    }

    func testExpressiveProfileAndNamedPost() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        app.buttons["You"].tap()
        app.buttons["Edit profile"].tap()
        XCTAssertTrue(app.navigationBars["Edit your profile"].waitForExistence(timeout: 10))
        capture("Profile editor lavender", app: app)
        app.cells["Add photo"].tap()
        let photo = app.images.matching(NSPredicate(format: "label BEGINSWITH 'Photo,'")).firstMatch
        XCTAssertTrue(photo.waitForExistence(timeout: 25), "Photos must finish loading its grid")
        // The remote Photos grid exposes the image frame before it exposes a hit point.
        let frame = photo.frame
        app.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
            .withOffset(CGVector(dx: frame.midX, dy: frame.midY)).tap()
        let crop = app.buttons["Use crop"]
        XCTAssertTrue(crop.waitForExistence(timeout: 12))
        let zoom = app.sliders["Zoom"]
        XCTAssertTrue(zoom.waitForExistence(timeout: 5))
        zoom.adjust(toNormalizedSliderPosition: 0.65)
        XCTAssertNotEqual(zoom.value as? String, "1.0×")
        let preview = app.descendants(matching: .any)["Profile photo crop"].firstMatch
        preview.swipeRight()
        capture("Choosing the profile photo crop", app: app)
        crop.tap()
        XCTAssertTrue(app.cells["Change photo"].waitForExistence(timeout: 10))
        let pronouns = app.textFields["Pronouns"]
        enter("they / them", into: pronouns, app: app)
        app.buttons["Dismiss keyboard"].tap()
        let bio = app.textViews.matching(NSPredicate(format: "label CONTAINS 'Bio'")).firstMatch
        reveal(bio, in: app)
        enter("Queer, here, and collecting tiny joys.", into: bio, app: app)
        app.buttons["Dismiss keyboard"].tap()
        let identity = app.textFields["Identity, in your words"]
        reveal(identity, in: app); enter("Queer", into: identity, app: app)
        app.buttons["Dismiss keyboard"].tap()
        reveal(app.cells["Chosen family"], in: app); app.cells["Chosen family"].tap()
        app.buttons["Save profile"].tap()
        XCTAssertTrue(app.staticTexts["they / them"].waitForExistence(timeout: 5))
        let avatar = app.images.matching(NSPredicate(format: "label BEGINSWITH %@", "Profile photo of ")).firstMatch
        XCTAssertTrue(avatar.exists)
        let postingName = String(avatar.label.dropFirst("Profile photo of ".count))
        capture("Saved expressive profile", app: app)
        app.terminate(); app.launchArguments = ["--netkit-ui-test"]; app.launch()
        app.buttons["You"].tap()
        XCTAssertTrue(app.staticTexts["Queer, here, and collecting tiny joys."].waitForExistence(timeout: 10))
        app.buttons["Post"].tap()
        writePost("A tiny joy for the neighborhood.", app: app)
        app.buttons["Dismiss keyboard"].tap()
        app.buttons["Post as Anonymous"].tap()
        app.descendants(matching: .any)["named-posting-identity"].firstMatch.tap()
        app.buttons["Post to nearby"].tap()
        XCTAssertFalse(avatar.exists, "The post feed has no profile photo.")
        capture("Named post with text-only byline", app: app)
        app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "View \(postingName)'s profile")).firstMatch.tap()
        XCTAssertTrue(app.staticTexts["they / them"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Shared with this post"].exists)
        XCTAssertTrue(app.staticTexts["Queer, here, and collecting tiny joys."].waitForExistence(timeout: 5))
        capture("Public author profile", app: app)
    }

    func testAppearanceEvidence() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        XCTAssertTrue(app.buttons["Post"].waitForExistence(timeout: 20))
        capture("Appearance feed", app: app)
        app.buttons["You"].tap()
        capture("Appearance profile", app: app)
        reveal(app.buttons["Edit profile"], in: app); app.buttons["Edit profile"].tap()
        XCTAssertTrue(app.navigationBars["Edit your profile"].waitForExistence(timeout: 5))
        capture("Appearance profile editor", app: app)
        let bio = app.textViews.matching(NSPredicate(format: "label CONTAINS 'Bio'")).firstMatch
        reveal(bio, in: app)
        enter("x", into: bio, app: app)
        app.typeText(XCUIKeyboardKey.delete.rawValue)
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Cancel"].isHittable)
        // Keyboard accessibility can appear before the presentation animation finishes.
        var previousKeyboardFrame = CGRect.zero
        var stableKeyboardSamples = 0
        let keyboardSettled = expectation(for: NSPredicate { _, _ in
            let keyboard = app.keyboards.firstMatch
            let frame = keyboard.frame
            stableKeyboardSamples = keyboard.exists && frame.height > 100 && frame == previousKeyboardFrame ? stableKeyboardSamples + 1 : 0
            previousKeyboardFrame = frame
            return stableKeyboardSamples >= 2
        }, evaluatedWith: app)
        wait(for: [keyboardSettled], timeout: 6)
        capture("Appearance profile keyboard", app: app)
        app.buttons["Cancel"].tap()
        app.buttons["Post"].tap()
        let headline = app.textViews["post-headline-input"]
        enter("A tiny joy", into: headline, app: app)
        XCTAssertFalse(app.buttons["Post to nearby"].isEnabled)
        capture("Appearance composer keyboard", app: app)
    }

    /// Scroll the table between its first and last fields while the navigation bar stays anchored.
    private func checkEditorScroll(_ app: XCUIApplication, lowerField: XCUIElement) {
        let cancel = app.buttons["Cancel"]
        XCTAssertTrue(cancel.waitForExistence(timeout: 5))
        reveal(cancel, in: app) // Finish sheet/keyboard presentation before measuring scroll motion.
        let initialY = cancel.frame.minY
        let origin = app.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
        func drag(from: CGFloat, to: CGFloat) {
            let keyboard = app.keyboards.firstMatch
            let keyboardTop = keyboard.exists && keyboard.frame.height > 100 && keyboard.frame.minY > cancel.frame.maxY + 60
                ? keyboard.frame.minY : app.frame.maxY
            // The floating Done toolbar intercepts touches above XCTest's keyboard frame.
            let done = app.buttons["Dismiss keyboard"]
            let accessoryTop = done.exists && done.isHittable ? done.frame.minY - 24 : keyboardTop
            let bottom = min(app.frame.maxY - 24, keyboardTop - 80, accessoryTop)
            let top = cancel.frame.maxY + 30
            let startY = max(top, min(app.frame.height * from, bottom))
            let endY = max(top, min(app.frame.height * to, bottom))
            // Start inside a table row. The screen edge belongs to UIKit's sheet gestures.
            let x = app.frame.width * 0.35
            origin.withOffset(CGVector(dx: x, dy: startY))
                .press(forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: x, dy: endY)), withVelocity: .default, thenHoldForDuration: 0.2)
            XCTAssertTrue(cancel.exists, "Scrolling must not dismiss the form")
            XCTAssertEqual(cancel.frame.minY, initialY, accuracy: 1, "Scrolling must not move the editor action row")
        }
        for _ in 0..<5 {
            if lowerField.exists && lowerField.isHittable { break }
            drag(from: 0.76, to: 0.30)
        }
        XCTAssertTrue(lowerField.exists && lowerField.isHittable, "The lower fields must scroll into view")
        drag(from: 0.76, to: 0.30)
        let firstField = app.textFields["Display name"]
        for _ in 0..<6 {
            if firstField.exists && firstField.isHittable { break }
            drag(from: 0.34, to: 0.78)
        }
        XCTAssertTrue(firstField.exists && firstField.isHittable)
        capture("Editor anchored after bidirectional swipes", app: app)
    }

    func testProfileEditorScrollingStaysAnchored() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        app.buttons["You"].tap(); app.buttons["Edit profile"].tap()
        XCTAssertTrue(app.navigationBars["Edit your profile"].waitForExistence(timeout: 5))
        checkEditorScroll(app, lowerField: app.textViews.matching(NSPredicate(format: "label CONTAINS 'Current joys'")).firstMatch)
        let pronouns = app.textFields["Pronouns"]
        reveal(pronouns, in: app); enter("they / them", into: pronouns, app: app)
        // Repeat while the keyboard is open; gestures stay above the keyboard frame.
        checkEditorScroll(app, lowerField: app.textViews.matching(NSPredicate(format: "label CONTAINS 'Current joys'")).firstMatch)
        let dismissKeyboard = app.buttons["Dismiss keyboard"]
        if dismissKeyboard.exists && dismissKeyboard.isHittable { dismissKeyboard.tap() }
        XCTAssertFalse(app.alerts["Discard profile changes?"].exists)
        app.buttons["Cancel"].tap()
        XCTAssertTrue(app.staticTexts["Discard profile changes?"].waitForExistence(timeout: 5))
        app.buttons["Keep editing"].tap(); app.buttons["Save profile"].tap()
        XCTAssertTrue(app.staticTexts["they / them"].waitForExistence(timeout: 5))
    }

    func testNativeComposerDismissalAndDiscard() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        let post = app.navigationBars.buttons["Post"]
        XCTAssertTrue(post.waitForExistence(timeout: 20))
        let sheetBar = app.navigationBars["New post"]
        let headline = app.textViews["post-headline-input"]
        let publish = sheetBar.buttons["Post to nearby"]
        let alert = app.alerts["Discard your draft?"]
        func pullSheetDown() {
            sheetBar.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                .press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.8)))
        }
        post.tap()
        XCTAssertTrue(sheetBar.waitForExistence(timeout: 5))
        XCTAssertFalse(publish.isEnabled)
        capture("Native new post sheet", app: app)
        pullSheetDown()
        XCTAssertTrue(sheetBar.waitForNonExistence(timeout: 5), "A clean sheet dismisses interactively")
        post.tap()
        enter("Keep this headlinx", into: headline, app: app)
        app.typeText(XCUIKeyboardKey.delete.rawValue + "e")
        XCTAssertEqual(headline.value as? String, "Keep this headline")
        XCTAssertFalse(publish.isEnabled)
        app.buttons["Dismiss keyboard"].tap()
        pullSheetDown()
        XCTAssertTrue(alert.waitForExistence(timeout: 5))
        capture("Native draft discard confirmation", app: app)
        alert.buttons["Keep writing"].tap()
        XCTAssertTrue(headline.waitForExistence(timeout: 5))
        XCTAssertEqual(headline.value as? String, "Keep this headline")
        let body = app.textViews["post-body-input"]
        let bodyText = "A walk in the park on Sunday?\nI can bring a picnic."
        enter(bodyText, into: body, app: app)
        XCTAssertEqual(body.value as? String, bodyText)
        XCTAssertTrue(publish.isEnabled && publish.isHittable)
        capture("Native composer ready to publish", app: app)
        sheetBar.buttons["Cancel"].tap()
        XCTAssertTrue(alert.waitForExistence(timeout: 5))
        alert.buttons["Discard"].tap()
        XCTAssertTrue(sheetBar.waitForNonExistence(timeout: 5))
        post.tap()
        XCTAssertTrue(sheetBar.waitForExistence(timeout: 5))
        XCTAssertNotEqual(headline.value as? String, "Keep this headline")
        XCTAssertNotEqual(body.value as? String, bodyText)
        XCTAssertFalse(publish.isEnabled)
        sheetBar.buttons["Cancel"].tap()
        XCTAssertTrue(sheetBar.waitForNonExistence(timeout: 5))
    }

    func testMultilineCommentIsRevealedAfterPosting() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        let post = app.buttons.matching(NSPredicate(format: "label CONTAINS 'A book club for the quietly social'")).firstMatch
        XCTAssertTrue(post.waitForExistence(timeout: 20))
        let comments = app.buttons["2 replies"]
        XCTAssertGreaterThanOrEqual(comments.frame.width, 44)
        XCTAssertGreaterThanOrEqual(comments.frame.height, 44)
        let bookmark = app.buttons["Save post"].firstMatch
        XCTAssertEqual(bookmark.frame.midX, app.buttons["Post options"].firstMatch.frame.midX, accuracy: 1)
        capture("Post with compact replies and trailing bookmark", app: app)
        comments.tap()
        let comment = app.textViews["reply-body"]
        XCTAssertTrue(app.staticTexts["Comments"].waitForExistence(timeout: 5))
        XCTAssertTrue(comment.exists)
        XCTAssertFalse(app.keyboards.firstMatch.exists)
        capture("Reading comments", app: app)
        comment.tap()
        XCTAssertFalse(app.buttons["Post comment"].isEnabled)
        let message = "Count me in for quiet company.\nI can bring a blanket and a very good book."
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        let identity = app.buttons["Reply as Anonymous"]
        XCTAssertTrue(identity.isHittable)
        app.typeText(message)
        XCTAssertEqual(comment.value as? String, message)
        identity.tap()
        app.buttons["Anonymous"].tap()
        XCTAssertEqual(comment.value as? String, message)
        XCTAssertTrue(app.buttons["Post comment"].isHittable)
        capture("Multiline comment draft with keyboard", app: app)
        app.buttons["Post comment"].tap()
        let posted = app.staticTexts[message]
        XCTAssertTrue(posted.waitForExistence(timeout: 8))
        let revealed = expectation(for: NSPredicate { _, _ in
            !app.keyboards.firstMatch.exists && posted.exists && posted.isHittable
                && posted.frame.minY >= 0 && comment.exists
        }, evaluatedWith: app)
        wait(for: [revealed], timeout: 10)
        XCTAssertFalse(app.buttons["Post comment"].exists)
        capture("New multiline comment revealed", app: app)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        let count = app.buttons["3 replies"].firstMatch
        reveal(count, in: app)
        count.tap()
        for _ in 0..<5 {
            if posted.exists && posted.isHittable { break }
            app.swipeUp()
        }
        XCTAssertTrue(posted.exists && posted.isHittable)
    }

    func testNearbyUsesOneSimulatedLocationAndPersistsRadius() throws {
        #if targetEnvironment(simulator)
        let device = XCUIDevice.shared
        let previous = device.location
        defer { device.location = previous }
        device.location = XCUILocation(location: CLLocation(latitude: 42.37, longitude: -71.11))
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        app.buttons["All cities"].tap()
        let panel = app.navigationBars["Your nearby feed"]
        XCTAssertTrue(panel.waitForExistence(timeout: 5))
        XCTAssertTrue(app.searchFields["search"].exists)
        capture("Nearby chooser", app: app)
        app.cells["locate"].tap()
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let allow = springboard.buttons["Allow While Using App"]
        if allow.waitForExistence(timeout: 4) { allow.tap() }
        let radius = app.textFields["radius"]
        XCTAssertTrue(radius.waitForExistence(timeout: 40))
        radius.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        let delete = XCUIKeyboardKey.delete.rawValue
        app.typeText(String(repeating: delete, count: 3))
        app.typeText("0")
        XCTAssertEqual(radius.value as? String, "0")
        XCTAssertTrue(panel.buttons["Apply"].wait(for: \.isEnabled, toEqual: false, timeout: 5))
        app.typeText(delete + "37")
        XCTAssertEqual(radius.value as? String, "37")
        XCTAssertTrue(panel.buttons["Apply"].wait(for: \.isEnabled, toEqual: true, timeout: 5))
        capture("Nearby GPS and mile radius", app: app)
        panel.buttons["Apply"].tap()
        let selected = app.buttons.matching(NSPredicate(format: "value == %@", "Within 37 miles")).firstMatch
        XCTAssertTrue(selected.waitForExistence(timeout: 5))
        app.terminate()
        app.launchArguments = ["--netkit-ui-test"]
        app.launch()
        XCTAssertTrue(selected.waitForExistence(timeout: 10))
        #else
        throw XCTSkip("Uses a simulated coordinate")
        #endif
    }

    func testNativeRootToolbarAndTabs() {
        let app = XCUIApplication()
        app.launchArguments = ["--netkit-ui-test", "--netkit-reset"]
        app.launch()
        let tabs = app.tabBars.firstMatch
        XCTAssertTrue(tabs.waitForExistence(timeout: 20))
        let inbox = app.navigationBars.buttons["Inbox"]
        let post = app.navigationBars.buttons["Post"]
        for name in ["Nearby", "Groups", "Saved", "You"] {
            let tab = tabs.buttons[name]
            tab.tap()
            XCTAssertTrue(tab.isSelected)
            XCTAssertTrue(inbox.isHittable)
            XCTAssertEqual(post.exists, name != "Groups")
            if name == "Groups" { capture("Groups with native toolbar and tabs", app: app) }
        }
        tabs.buttons["Nearby"].tap()
        inbox.tap()
        let back = app.navigationBars.buttons.element(boundBy: 0)
        XCTAssertTrue(back.waitForExistence(timeout: 5))
        XCTAssertFalse(inbox.exists)
        XCTAssertFalse(post.exists)
        app.navigationBars.buttons["Inbox settings"].tap()
        app.buttons["Requests"].tap()
        XCTAssertTrue(app.staticTexts["No message requests."].waitForExistence(timeout: 5))
        back.tap()
        XCTAssertTrue(app.navigationBars.buttons["Inbox settings"].waitForExistence(timeout: 5))
        back.tap()
        XCTAssertTrue(inbox.waitForExistence(timeout: 5))
        XCTAssertTrue(post.isHittable)
        XCTAssertTrue(tabs.buttons["Nearby"].isSelected)
        XCTAssertTrue(app.buttons["All cities"].exists)
        capture("Nearby with native toolbar and tabs", app: app)
    }
}
