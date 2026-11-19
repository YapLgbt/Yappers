# Handling a safety report

Reporting is opt-in per primary home. Enable it when a moderator can check the queue and respond. Assign the primary process an absolute `YAP_REPORT_INBOX` path inside a private, owner-only directory. Use a separate file for each home. Exclude that directory from backups, filesystem snapshots, crash dumps and log collection. Keep the inbox on an encrypted volume. It contains the submitted evidence in readable form for the operator. The server creates the file with mode 0600 and rejects group- or world-accessible files.

Clients contact the enrolled primary directly. If that route is unavailable, the client can retry with the same receipt. Sending a report does not block its subject. The receipt contains the primary home ID, a draft timestamp and a random 256-bit capability. Its holder can check the outcome. A moderator with the database can read a case but cannot derive the private receipt from the case ID.

## Review a case

Run these commands as the service user on the primary, in a private terminal. A service account with access to this file is a moderator. The network protocol has no operator queue or case-management command.

```sh
yappers-server reports --inbox /private/reports.sqlite list
yappers-server reports --inbox /private/reports.sqlite show CASE_ID
yappers-server reports --inbox /private/reports.sqlite close CASE_ID --outcome noAction
```

The queue lists case IDs and receipt times. Open an individual case to read the submitted category, subject type, optional identifier and excerpt. Treat the evidence as a reporter’s account of what happened. An excerpt can be edited, and an account ID alone cannot establish who wrote it. Check the context available to your home without requesting unrelated message history or a recovery key. Take any warranted action with the home’s existing moderation tools, then close with `actionTaken`. Closing a case records the outcome and clears the evidence. It does not itself hide a post or restrict an account. The reporter sees only the outcome. Keep excerpts out of shell logs, incident tickets and notifications.

| Record | Where it goes | When it is removed |
|---|---|---|
| Reason, subject type, selected text, optional subject ID | Operator’s private inbox | On closure, or after seven days if still open |
| Receipt digest, submission time, disposition and closure time | Same inbox | Thirty days after submission |
| Plain receipt | Reporter; presented directly to the primary to submit or check status | The app displays it for the person to save; the server stores only its digest |

The primary purges on access, on startup and once per minute while serving. A stopped machine cannot clean its disk until it runs again. Reports are excluded from the feed DB, standby ledger, protected feed backups and federation. A machine-level snapshot or a moderator copying text creates a separate copy. Operators must account for those copies themselves. The transport can see connections; text or selected identifiers can identify the reporter. This workflow does not promise anonymity.

A flood can exhaust the home-wide intake limit (60 new cases per hour, 2,000 open cases). Existing receipt holders can still check status. Receipt lookup and retries never return submitted evidence. A new draft can first be submitted within one hour. Keep reports disabled on an unstaffed home. Operators still need a way to receive urgent or alternate contact when their home is down.
