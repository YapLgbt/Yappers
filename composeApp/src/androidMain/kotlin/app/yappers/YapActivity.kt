package app.yappers

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.*

@Composable
internal actual fun AppBackHandler(enabled: Boolean, onBack: () -> Unit) = BackHandler(enabled, onBack)

class YapActivity : ComponentActivity() {
    private val yap get() = application as YapApplication
    private var foreground by mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val files = AndroidFiles(this)
        val nearby = AndroidNearby(this)
        val preferences = getSharedPreferences("installation", MODE_PRIVATE)
        setContent {
            var welcome by remember { mutableStateOf(!preferences.getBoolean("welcomed", false)) }
            YappersApp(yap.storage.local, yap.controller, foreground = foreground,
                photoLibrary = files, recoveryFiles = files, nearbyServices = nearby,
                installationWelcome = welcome, onWelcomeComplete = {
                    preferences.edit().putBoolean("welcomed", true).apply()
                    welcome = false
                })
        }
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
