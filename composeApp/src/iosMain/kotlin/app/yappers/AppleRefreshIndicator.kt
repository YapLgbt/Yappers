@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class, androidx.compose.ui.ExperimentalComposeUiApi::class)

package app.yappers

import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.UIKitInteropProperties
import androidx.compose.ui.viewinterop.UIKitView
import platform.UIKit.UIView

interface AppleRefreshIndicator {
    val view: UIView
    fun updatePull(progress: Double, refreshing: Boolean)
}

@Composable
internal fun AppleRefreshIndicatorView(factory: AppleEditorFactory, progress: Float, refreshing: Boolean) {
    val indicator = remember(factory) { factory.makeRefreshIndicator() }
    UIKitView(factory = { indicator.view }, modifier = Modifier.size(28.dp),
        update = { indicator.updatePull(progress.toDouble(), refreshing) },
        properties = UIKitInteropProperties(interactionMode = null, placedAsOverlay = true))
}
