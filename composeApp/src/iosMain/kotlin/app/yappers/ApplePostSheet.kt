@file:OptIn(kotlinx.cinterop.ExperimentalForeignApi::class, kotlinx.cinterop.BetaInteropApi::class)

package app.yappers

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.uikit.LocalUIViewController
import kotlinx.cinterop.ObjCAction
import platform.Foundation.NSSelectorFromString
import platform.UIKit.*
import platform.darwin.NSObject

@Composable
internal fun ApplePostSheet(model: PostForm, factory: AppleEditorFactory) {
    val host = LocalUIViewController.current
    val editor = remember(host, factory) {
        PostEditorPresentation(factory.makePostForm(model), model)
    }
    SideEffect { editor.update(model) }
    DisposableEffect(host, editor) {
        editor.navigation.view.tintColor = host.view.tintColor
        host.presentViewController(editor.navigation, animated = true, completion = null)
        onDispose {
            if (!editor.navigation.beingDismissed) {
                editor.navigation.presentingViewController?.dismissViewControllerAnimated(true, completion = null)
            }
        }
    }
}

private class PostEditorPresentation(private val form: ApplePostFormView, private var actions: PostForm) :
    NSObject(), UIAdaptivePresentationControllerDelegateProtocol {
    val navigation = UINavigationController(rootViewController = form.controller)
    // UIKit uses white titles on prominent actions in both appearances.
    private val filledActionTint = LightColors.primary.let {
        UIColor.colorWithRed(it.red.toDouble(), it.green.toDouble(), it.blue.toDouble(), 1.0)
    }
    private val cancel = UIBarButtonItem(title = "Cancel", style = UIBarButtonItemStyle.UIBarButtonItemStylePlain,
        target = this, action = NSSelectorFromString("requestDismiss"))
    private val publish = UIBarButtonItem(title = "Post", style = UIBarButtonItemStyle.UIBarButtonItemStyleDone,
        target = this, action = NSSelectorFromString("publishPost"))

    @ObjCAction
    fun publishPost() {
        if (actions.canPublish) actions.publish()
    }

    init {
        publish.tintColor = filledActionTint
        form.controller.navigationItem.title = "New post"
        form.controller.navigationItem.leftBarButtonItem = cancel
        form.controller.navigationItem.rightBarButtonItem = publish
        navigation.modalPresentationStyle = UIModalPresentationPageSheet
        navigation.sheetPresentationController?.prefersGrabberVisible = true
        navigation.presentationController?.delegate = this
    }

    fun update(next: PostForm) {
        form.renderForm(next)
        actions = next
        cancel.enabled = !next.submitting
        publish.enabled = next.canPublish
        publish.title = if (next.submitting) "Saving…" else "Post"
        publish.accessibilityLabel = next.publishLabel
    }

    override fun presentationControllerShouldDismiss(presentationController: UIPresentationController) =
        !actions.submitting && !actions.draft.dirty

    override fun presentationControllerDidAttemptToDismiss(presentationController: UIPresentationController) {
        requestDismiss()
    }

    override fun presentationControllerDidDismiss(presentationController: UIPresentationController) {
        actions.dismiss()
    }

    @ObjCAction
    fun requestDismiss() {
        if (actions.submitting || navigation.presentedViewController != null) return
        if (!actions.draft.dirty) {
            actions.dismiss()
            return
        }
        val alert = UIAlertController.alertControllerWithTitle("Discard your draft?", "Your words won't be saved.", UIAlertControllerStyleAlert)
        val keep = UIAlertAction.actionWithTitle("Keep writing", UIAlertActionStyleCancel, null)
        alert.addAction(keep)
        alert.addAction(UIAlertAction.actionWithTitle("Discard", UIAlertActionStyleDestructive) { actions.dismiss() })
        alert.preferredAction = keep
        alert.view.tintColor = filledActionTint
        navigation.presentViewController(alert, animated = true, completion = null)
    }
}
