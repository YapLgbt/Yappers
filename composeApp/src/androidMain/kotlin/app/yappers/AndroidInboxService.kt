package app.yappers

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.activity.result.contract.ActivityResultContracts
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

private const val channelId = "inbox"
private fun Context.manager() = getSystemService(NotificationManager::class.java)
private fun Context.preferences() = getSharedPreferences("push-delivery", Context.MODE_PRIVATE)
private fun Context.createInboxChannel() =
    manager().createNotificationChannel(NotificationChannel(channelId, "Inbox", NotificationManager.IMPORTANCE_DEFAULT))

internal class AndroidPush(val activity: YapActivity) : PushRegistration {
    companion object {
        fun configured(context: Context) = FirebaseApp.getApps(context).isNotEmpty()
    }
    override val provider = "fcm"
    override val currentToken get() = activity.preferences().getString("token", null)
    private var callback: ((String?, String?) -> Unit)? = null
    private val permission = activity.registerForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        if (allowed) fetch() else finish(null, "Allow notifications in Android settings to receive Inbox updates.")
    }
    private fun finish(token: String?, error: String?) {
        val complete = callback
        callback = null
        complete?.invoke(token, error)
    }
    private fun fetch() {
        if (!activity.manager().areNotificationsEnabled() ||
            activity.manager().getNotificationChannel(channelId)?.importance == NotificationManager.IMPORTANCE_NONE) {
            finish(null, "Allow Inbox notifications in Android settings.")
            return
        }
        FirebaseMessaging.getInstance().token.addOnCompleteListener { result ->
            if (result.isSuccessful) {
                activity.preferences().edit().putString("token", result.result).apply()
                finish(result.result, null)
            } else finish(null, "Couldn’t register this device. Try again.")
        }
    }
    override fun requestToken(completion: (String?, String?) -> Unit) {
        activity.runOnUiThread {
            callback = completion
            activity.createInboxChannel()
            if (Build.VERSION.SDK_INT >= 33 && activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED) permission.launch(Manifest.permission.POST_NOTIFICATIONS)
            else fetch()
        }
    }
}

class AndroidInboxService : FirebaseMessagingService() {
    companion object { internal const val OPEN = "app.yappers.OPEN_INBOX" }

    override fun onNewToken(token: String) {
        preferences().edit().putString("token", token).apply()
    }
    override fun onMessageReceived(message: RemoteMessage) {
        val route = message.data["route"]?.takeIf { Regex("[a-f0-9]{64}").matches(it) } ?: return
        val revision = message.data["revision"]?.toLongOrNull() ?: return
        createInboxChannel()
        if (!manager().areNotificationsEnabled()) return
        val existing = manager().activeNotifications.firstOrNull { it.tag == route }
        if ((existing?.notification?.extras?.getLong("revision") ?: 0L) >= revision) return
        val intent = Intent(this, YapActivity::class.java).apply {
            action = OPEN
            data = Uri.fromParts("yap-inbox", route, null)
            putExtra("route", route)
            flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val tap = PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification = Notification.Builder(this, channelId)
            .setSmallIcon(app.yappers.shared.R.drawable.ic_inbox_notification)
            .setContentTitle("Yap").setContentText("You have an Inbox update.")
            .setContentIntent(tap).setAutoCancel(true).setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PRIVATE).build()
        notification.extras.putLong("revision", revision)
        manager().notify(route, 1, notification)
    }
}
