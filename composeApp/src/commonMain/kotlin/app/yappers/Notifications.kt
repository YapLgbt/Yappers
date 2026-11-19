package app.yappers

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.Serializable
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

@Serializable
data class InboxItem(val target: String, val through: Long, val unread: Int, val created: Long,
                     val scope: String = "", val title: String = "", val first: String = "", val last: String = "")

@Serializable
data class NotificationInbox(val activity: List<InboxItem> = emptyList(), val messages: List<InboxItem> = emptyList(),
                             val requests: List<InboxItem> = emptyList()) {
    val count get() = activity.count { it.unread > 0 } + messages.count { it.unread > 0 } + requests.size
    fun read(category: String, target: String, through: Long): NotificationInbox {
        fun List<InboxItem>.marked() = map { if (it.target == target && it.through <= through) it.copy(unread = 0) else it }
        return if (category == "reply") copy(activity = activity.marked()) else copy(messages = messages.marked())
    }
}

interface PushRegistration {
    val provider: String
    val currentToken: String?
    fun requestToken(completion: (String?, String?) -> Unit)
}

object InboxPush {
    var registration: PushRegistration? = null
    private val pending = MutableStateFlow<String?>(null)
    internal val routes = pending.asStateFlow()
    fun open(route: String) { if (route.length == 64 && route.all { it in "0123456789abcdef" }) pending.value = route }
    internal fun take(route: String) = pending.compareAndSet(route, null)
    internal suspend fun token(): String = suspendCancellableCoroutine { continuation ->
        checkNotNull(registration) { "Push is unavailable on this device." }.requestToken { token, error ->
            if (continuation.isActive) {
                if (!token.isNullOrBlank()) continuation.resume(token)
                else continuation.resumeWithException(IllegalStateException(error ?: "Notifications are unavailable."))
            }
        }
    }
}
