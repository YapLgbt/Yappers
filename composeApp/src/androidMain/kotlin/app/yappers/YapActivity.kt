package app.yappers

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import com.google.firebase.messaging.FirebaseMessaging
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.*

@Composable
internal actual fun AppBackHandler(enabled: Boolean, onBack: () -> Unit) = BackHandler(enabled, onBack)

class YapActivity : ComponentActivity() {
    private val yap get() = application as YapApplication
    private var foreground by mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (AndroidPush.configured(this)) {
            InboxPush.registration = AndroidPush(this)
            lifecycleScope.launch {
                yap.controller.state.map { it.privacy.pushCategories.isNotEmpty() }.distinctUntilChanged().collect { enabled ->
                    FirebaseMessaging.getInstance().isAutoInitEnabled = enabled
                }
            }
        }
        acceptInboxIntent(intent)
        enableEdgeToEdge()
        val files = AndroidFiles(this)
        val nearby = AndroidNearby(this)
        val preferences = getSharedPreferences("installation", MODE_PRIVATE)
        setContent {
            var welcome by remember { mutableStateOf(!preferences.getBoolean("welcomed", false)) }
            CompositionLocalProvider(LocalMaterialNavigation provides AndroidNavigation) {
                YappersApp(yap.storage.local, yap.controller, foreground = foreground,
                    photoLibrary = files, recoveryFiles = files, nearbyServices = nearby,
                    installationWelcome = welcome, onWelcomeComplete = {
                        preferences.edit().putBoolean("welcomed", true).apply()
                        welcome = false
                    })
            }
        }
    }

    private fun acceptInboxIntent(intent: Intent) {
        if (intent.action == AndroidInboxService.OPEN) {
            AndroidPush.ifActive(this) { intent.getStringExtra("route")?.let(InboxPush::open) }
            intent.removeExtra("route")
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        acceptInboxIntent(intent)
    }

    override fun onDestroy() {
        if ((InboxPush.registration as? AndroidPush)?.activity === this) InboxPush.registration = null
        super.onDestroy()
    }

    override fun onStart() {
        super.onStart()
        yap.controller.setForeground(true)
        foreground = true
    }

    override fun onStop() {
        foreground = false
        yap.leaveForeground()
        super.onStop()
    }
}
