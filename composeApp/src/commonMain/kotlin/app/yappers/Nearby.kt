package app.yappers

import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.*
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlin.math.*

/** Shared signed location representation. One unit is one hundredth of a degree. */
@Serializable
data class PostArea(val label: String, val lat100: Int, val lon100: Int) {
    init {
        require(label.isNotBlank() && label.length <= 160 && lat100 in -9000..9000 && lon100 in -18000..18000)
    }

    fun milesTo(other: PostArea): Double {
        val a = lat100 * PI / 18000
        val b = other.lat100 * PI / 18000
        val h = sin((b - a) / 2).pow(2) + cos(a) * cos(b) * sin((lon100 - other.lon100) * PI / 36000).pow(2)
        return 7917.52 * asin(sqrt(h.coerceIn(0.0, 1.0)))
    }
}

@Serializable
data class NearbyPreference(val center: PostArea? = null, val radiusMiles: Int = 25) {
    init { require(radiusMiles in 1..250) }
    fun includes(area: PostArea?): Boolean = center == null || (area != null && center.milesTo(area) <= radiusMiles)
}

/** Apple platform services return coarse places, or the public enrollment document. */
interface NearbyServices {
    fun request(operation: String, query: String, onResult: (String?, String?) -> Unit)
}

internal suspend fun NearbyServices.perform(operation: String, query: String = ""): String =
    suspendCancellableCoroutine { continuation ->
        request(operation, query) { value, error ->
            if (continuation.isActive) {
                if (value != null) continuation.resume(value)
                else continuation.resumeWithException(IllegalStateException(error ?: "Please try again."))
            }
        }
    }

internal data class PublicEnrollment(val join: JsonObject, val replica: JsonObject?, val fullHomeStandby: JsonObject?)
internal fun publicHomeJoin(document: String): String = publicEnrollment(document).join.toString()

internal fun publicEnrollment(document: String): PublicEnrollment {
    require(document.length <= 16_384)
    val manifest = Json.parseToJsonElement(document).jsonObject
    require(manifest["version"]?.jsonPrimitive?.intOrNull == 1)
    val join = manifest.getValue("join").jsonObject
    val home = join.getValue("serverAddr").jsonObject.getValue("id").jsonPrimitive.content
    require(home.matches(Regex("[0-9a-f]{64}")) && join["community"]?.jsonPrimitive?.content == "$home/public" &&
        join["networkMode"]?.jsonPrimitive?.content == "n0") { "Invalid public home enrollment." }
    val replica = manifest["publicFeedReplica"]?.jsonObject
    if (replica != null) {
        val id = replica["id"]?.jsonPrimitive?.content.orEmpty()
        require(id.matches(Regex("[0-9a-f]{64}")) && id != home) { "Invalid public feed replica." }
    }
    val standby = manifest["fullHomeStandby"]?.jsonObject
    if (standby != null) {
        require(standby["home"]?.jsonPrimitive?.content == home) { "Standby belongs to another home." }
    }
    return PublicEnrollment(join, replica, standby)
}
