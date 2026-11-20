@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class, kotlinx.cinterop.BetaInteropApi::class)

package app.yappers

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.uikit.LocalUIViewController
import androidx.compose.ui.window.ComposeUIViewController
import kotlinx.cinterop.ObjCAction
import platform.Foundation.NSSelectorFromString
import platform.UIKit.*
import platform.darwin.NSObject

@Composable
internal fun AppleProfileSheet(factory: AppleEditorFactory, onDismiss: () -> Unit, content: @Composable () -> Unit) {
    val host = LocalUIViewController.current
    val density by rememberUpdatedState(LocalDensity.current)
    val latestContent by rememberUpdatedState(content)
    val dismiss by rememberUpdatedState(onDismiss)
    val sheet = remember(host, factory) {
        val controller = ComposeUIViewController {
            CompositionLocalProvider(
                LocalDensity provides density,
                LocalFeedControl provides { row, modifier -> AppleFeedButton(factory, row, modifier) },
            ) {
                AppleAppearance {
                    Surface(Modifier.fillMaxSize()) {
                        Box(Modifier.fillMaxSize().safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
                            latestContent()
                        }
                    }
                }
            }
        }
        ProfilePresentation(controller) { dismiss() }
    }
    DisposableEffect(host, sheet) {
        sheet.navigation.view.tintColor = host.tabBarController?.view?.tintColor ?: host.view.tintColor
        host.presentViewController(sheet.navigation, animated = true, completion = null)
        onDispose {
            if (!sheet.navigation.beingDismissed) {
                sheet.navigation.presentingViewController?.dismissViewControllerAnimated(true, completion = null)
            }
        }
    }
}

private class ProfilePresentation(controller: UIViewController, private val dismiss: () -> Unit) :
    NSObject(), UIAdaptivePresentationControllerDelegateProtocol {
    val navigation = UINavigationController(rootViewController = controller)

    init {
        controller.navigationItem.rightBarButtonItem = UIBarButtonItem(
            title = "Done", style = UIBarButtonItemStyle.UIBarButtonItemStylePlain,
            target = this, action = NSSelectorFromString("closeProfile"),
        )
        navigation.modalPresentationStyle = UIModalPresentationPageSheet
        navigation.sheetPresentationController?.prefersGrabberVisible = true
        navigation.presentationController?.delegate = this
    }

    @ObjCAction
    fun closeProfile() = dismiss()

    override fun presentationControllerDidDismiss(presentationController: UIPresentationController) = dismiss()
}
