package app.yappers

import platform.UIKit.UIView
import platform.UIKit.UIViewController

interface AppleEditorFactory {
    fun makeFormSurface(mode: String, onHeight: (Double) -> Unit): AppleFormSurface
    fun showConfirmation(host: UIViewController, confirmation: FormConfirmation): UIViewController
    fun makePhotoCrop(model: NativePhotoCrop): UIViewController
    fun makeFeedControl(): AppleFeedControl
    fun makePostForm(model: PostForm): ApplePostFormView
    fun makeInlineEditor(controller: UIViewController, onHeightChange: (Double) -> Unit): AppleInlineEditor
}

interface ApplePostFormView {
    val controller: UIViewController
    fun renderForm(model: PostForm)
}

interface AppleInlineEditor {
    val view: UIView
    fun render(model: InlineEditorModel)
}


interface AppleFormSurface {
    val controller: UIViewController
    fun updateSurface(form: NativeFormModel)
    fun showSheet(host: UIViewController)
    fun closeSheet()
}

@androidx.compose.runtime.Composable
internal fun ApplePhotoCropSheet(factory: AppleEditorFactory, crop: NativePhotoCrop) {
    val host = androidx.compose.ui.uikit.LocalUIViewController.current
    androidx.compose.runtime.DisposableEffect(host, crop) {
        var presenter = host.view.window?.rootViewController ?: host
        while (true) presenter = presenter.presentedViewController ?: break
        val cropPage = factory.makePhotoCrop(crop)
        val navigation = platform.UIKit.UINavigationController(rootViewController = cropPage)
        // The presenting Compose view observes the result and dismisses this controller.
        // Keep it in the window while the opaque crop editor covers it.
        navigation.modalPresentationStyle = platform.UIKit.UIModalPresentationOverFullScreen
        navigation.view.tintColor = host.view.tintColor
        presenter.presentViewController(navigation, animated = true, completion = null)
        onDispose {
            if (navigation.presentingViewController != null && !navigation.beingDismissed)
                navigation.dismissViewControllerAnimated(true, completion = null)
        }
    }
}
