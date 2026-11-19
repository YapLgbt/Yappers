package app.yappers.qa;

import android.app.Activity;
import android.app.Instrumentation;
import android.os.Build;
import android.os.Bundle;
import android.util.Base64;
import app.yappers.*;
import java.nio.charset.StandardCharsets;
import kotlin.coroutines.Continuation;
import kotlin.coroutines.EmptyCoroutineContext;
import kotlin.jvm.functions.Function1;
import kotlinx.coroutines.BuildersKt;
import kotlinx.serialization.json.JsonObject;
import kotlinx.serialization.json.Json;
import kotlinx.serialization.json.JsonPrimitive;
import org.json.JSONObject;
import java.util.Collections;

/** Runs in a separate test APK, with a new account on the local QA home only. */
public final class DisposableHomeCheck extends Instrumentation {
    private Bundle arguments;

    @Override public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        this.arguments = arguments;
        start();
    }

    private static <T> T await(Function1<Continuation<? super T>, Object> call) throws InterruptedException {
        return BuildersKt.runBlocking(EmptyCoroutineContext.INSTANCE, (scope, continuation) -> call.invoke(continuation));
    }
    private static void require(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }

    @Override public void onStart() {
        Bundle result = new Bundle();
        String stage = "fixture";
        try {
            require(Build.MODEL.contains("sdk") || Build.HARDWARE.contains("ranchu"), "Use a disposable emulator");
            if ("public-discovery".equals(arguments.getString("check"))) {
                stage = "public enrollment discovery";
                android.content.Intent launch = getTargetContext().getPackageManager().getLaunchIntentForPackage(getTargetContext().getPackageName());
                Activity activity = startActivitySync(launch);
                try {
                    waitForIdleSync();
                    NearbyKt.publicHomeJoin(YapApplicationKt.fetchEnrollment());
                } finally { runOnMainSync(activity::finish); }
                result.putString("checks", "Live enrollment address reachable and valid");
                finish(Activity.RESULT_OK, result);
                return;
            }
            String manifest = new String(Base64.decode(arguments.getString("fixture"), Base64.DEFAULT), StandardCharsets.UTF_8);
            JSONObject join = new JSONObject(manifest).getJSONObject("join");
            require(join.getJSONObject("serverAddr").getString("id").equals(arguments.getString("expectedHome")), "Wrong fixture");
            require(!join.getJSONObject("serverAddr").getString("id").equals("114f92b50e66dd84a14e0ff9b857b0c254565609b00b0cb9ef85f29e864efe5b"), "Refusing production");
            AndroidStorage store = new AndroidStorage(getTargetContext());
            NetkitController client = new NetkitController(store, new AndroidNetkitEngine(), ignored -> manifest);
            client.prepareWelcome$Yappers_composeApp(false);
            require(client.getWelcome$Yappers_composeApp().getValue(), "A clean tester installation is required");
            client.setForeground(true);
            stage = "first-run enrollment";
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.finishWelcome$Yappers_composeApp("Android reader", null, null, false, c));
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.checkWelcomeUsername$Yappers_composeApp(false, c));
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.claimUsername$Yappers_composeApp("android_reader", c));
            DisposableHomeCheck.<kotlin.Unit>await(client::enterNeighborhood$Yappers_composeApp);
            DisposableHomeCheck.<kotlin.Unit>await(client::sync);
            require(client.getState().getValue().getOnline(), "Home offline");
            stage = "claim username and publish";
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.publish("See you there", "Chatter", false, "Android reader", "Android meetup", new PostArea("Test town", 4020,-7300), c));
            DisposableHomeCheck.<kotlin.Unit>await(client::sync);
            Post own = client.getState().getValue().getSnapshot().getPosts().stream().filter(p -> p.getHeadline().equals("Android meetup")).findFirst().get();
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.reply(own.getId(), "Bring a book", true, c));
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.save(own.getId(), c));
            DisposableHomeCheck.<kotlin.Unit>await(client::sync);
            Post other = client.getState().getValue().getSnapshot().getPosts().stream().filter(p -> p.getHeadline().equals("Native inbox test")).findFirst().get();
            stage = "send and receive encrypted DM";
            MessageContact contact = await(c -> client.messageContact(other.getRemoteId(), c));
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.sendMessage(contact, "Android says hello 🌈", c));
            boolean received = false;
            for (int attempt = 0; attempt < 45 && !received; attempt++) {
                DisposableHomeCheck.<kotlin.Unit>await(client::sync);
                received = client.getState().getValue().getMessages().stream().anyMatch(m -> !m.getOutgoing() && m.getBody().equals("Hello from native 🌈"));
                if (!received) Thread.sleep(1000);
            }
            require(received, "No reply from native peer");
            stage = "Inbox read progress";
            InboxItem unread = client.getState().getValue().getPrivacy().getNotifications().getMessages().stream()
                .filter(row -> row.getTarget().equals(contact.getPeer()) && row.getUnread()>0).findFirst().get();
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.readNotification("message", unread, c));
            DisposableHomeCheck.<kotlin.Unit>await(client::sync);
            require(client.getState().getValue().getPrivacy().getNotifications().getMessages().isEmpty(), "DM read mark did not persist");
            stage = "report";
            JsonObject draft = DisposableHomeCheck.<JsonObject>await(client::reportDraft$Yappers_composeApp);
            String receipt = ((JsonPrimitive)draft.get("receipt")).getContent();
            JsonObject evidence = (JsonObject)Json.Default.parseToJsonElement("{\"subject\":\"post\",\"category\":\"other\",\"text\":\"Disposable QA report\"}");
            JsonObject status = await(c -> client.submitReport$Yappers_composeApp(receipt, evidence, c));
            require(((JsonPrimitive)status.get("status")).getContent().equals("received"), "Missing moderation receipt");
            stage = "block and unblock";
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.blockAccount(other.getRemoteId(), true, "Native reader", contact.getPeer(), c));
            DisposableHomeCheck.<kotlin.Unit>await(client::sync);
            require(client.getState().getValue().getSnapshot().getPosts().stream().noneMatch(p -> p.getId() == other.getId()), "Blocked post is visible");
            DisposableHomeCheck.<kotlin.Unit>await(client::refreshBlocks);
            String block = client.getState().getValue().getBlockedAccounts().get(0).getId();
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.unblockAccount(block, c));
            stage = "create group";
            DisposableHomeCheck.<kotlin.Unit>await(c -> client.createGroup("Book club", "Discuss our reading", "Friends", null, c));
            require(client.getState().getValue().getActiveGroup().getName().equals("Book club"), "Group did not open");
            DisposableHomeCheck.<kotlin.Unit>await(client::openHomeFeed);
            stage = "encrypted backup";
            String backup = await(c -> client.exportRecovery("a disposable test passphrase", new JsonObject(Collections.emptyMap()), false, c));
            String recovered = await(c -> client.keyFromBackup$Yappers_composeApp(backup, "a disposable test passphrase", c));
            String key = DisposableHomeCheck.<String>await(client::recoveryKey$Yappers_composeApp);
            require(key.equals(recovered), "Backup failed to restore account key");
            stage = "cold background refresh";
            DisposableHomeCheck.<kotlin.Unit>await(client::shutdown);
            AndroidStorage reopened = new AndroidStorage(getTargetContext());
            NetkitController cold = new NetkitController(reopened, new AndroidNetkitEngine(), ignored -> manifest);
            cold.setForeground(false);
            Boolean refreshed = await(c -> cold.backgroundRefresh(new NearbyPreference(null, 25), c));
            require(Boolean.TRUE.equals(refreshed), "Background refresh did not complete");
            result.putString("checks", "Enrollment, username, post, reply, bookmark, two-way DM, report, block, group, backup, cold background sync passed");
            finish(Activity.RESULT_OK, result);
        } catch (Throwable failure) {
            result.putString("failedStage", stage);
            result.putString("reason", failure.getClass().getSimpleName() + ": " + failure.getMessage());
            finish(Activity.RESULT_CANCELED, result);
        }
    }
}
