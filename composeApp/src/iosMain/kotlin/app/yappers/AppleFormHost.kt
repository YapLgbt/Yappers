@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)

package app.yappers

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.uikit.LocalUIViewController
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.UIKitInteropInteractionMode
import androidx.compose.ui.viewinterop.UIKitInteropProperties
import androidx.compose.ui.viewinterop.UIKitViewController

@OptIn(androidx.compose.ui.ExperimentalComposeUiApi::class)
@Composable
internal fun AppleFormHost(factory: AppleEditorFactory, model: NativeFormModel, mode: String, modifier: Modifier) {
    val host = LocalUIViewController.current
    var height by remember { mutableStateOf(44.dp) }
    val form = remember(factory, host, mode) { factory.makeFormSurface(mode) { height = it.dp } }
    SideEffect { form.updateSurface(model) }
    if (mode == "sheet" || mode == "compactSheet") {
        DisposableEffect(form, host) {
            form.showSheet(host)
            onDispose { form.closeSheet() }
        }
    } else {
        val inline = mode == "inline" || mode == "input"
        UIKitViewController(factory = { form.controller },
            modifier = if (inline) modifier.fillMaxWidth().height(height) else modifier,
            properties = UIKitInteropProperties(
                interactionMode = if (inline) UIKitInteropInteractionMode.Cooperative() else UIKitInteropInteractionMode.NonCooperative,
                isNativeAccessibilityEnabled = true, placedAsOverlay = inline))
    }
}

@Composable
internal fun AppleConfirmation(factory: AppleEditorFactory, model: FormConfirmation) {
    val host = LocalUIViewController.current
    val latest by rememberUpdatedState(model)
    DisposableEffect(host, model.title, model.message) {
        val alert = factory.showConfirmation(host, FormConfirmation(model.title, model.message,
            FormChoice(model.confirm.title, destructive = model.confirm.destructive) { latest.confirm.select() },
            FormChoice(model.cancel.title) { latest.cancel.select() }))
        onDispose { if (!alert.beingDismissed && alert.presentingViewController != null) alert.dismissViewControllerAnimated(false, null) }
    }
}
