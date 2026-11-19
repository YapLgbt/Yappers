package app.yappers

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.location.LocationManager
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.location.LocationManagerCompat
import androidx.core.os.CancellationSignal
import androidx.lifecycle.lifecycleScope
import java.text.Normalizer
import java.util.zip.GZIPInputStream
import kotlinx.coroutines.*
import kotlinx.serialization.json.Json
import kotlin.coroutines.resume
import kotlin.math.roundToInt

internal class AndroidNearby(private val activity: ComponentActivity) : NearbyServices, OfflineTownCatalog {
    private var awaitingPermission: ((String?, String?) -> Unit)? = null
    private val permission = activity.registerForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        val callback = awaitingPermission
        awaitingPermission = null
        if (callback != null) {
            if (allowed) request("locate", "", callback)
            else callback(null, "Search for a city or town to choose your area.")
        }
    }
    private var searchJob: Job? = null
    private val accents = Regex("\\p{M}+")
    private fun folded(text: String) = Normalizer.normalize(text.lowercase(), Normalizer.Form.NFD).replace(accents, "")
    private suspend fun towns(search: String?, center: PostArea?): List<PostArea> {
        val results = mutableListOf<PostArea>()
        val parts = search?.let(::folded)?.split(',', ' ')?.filter(String::isNotBlank).orEmpty()
        var nearest: PostArea? = null
        var distance = 30.0
        val job = currentCoroutineContext()[Job]
        GZIPInputStream(activity.assets.open("towns.bin")).bufferedReader().useLines { lines ->
            for (line in lines) {
                job?.ensureActive()
                val fields = line.split('\t')
                if (fields.size != 5) continue
                val lat = fields[3].toInt()
                val lon = fields[4].toInt()
                if (center != null && kotlin.math.abs(lat - center.lat100) > 45) continue
                val label = fields.take(3).filter(String::isNotBlank).distinct().joinToString(", ")
                if (label.length > 160) continue
                if (center == null) {
                    val name = folded(label)
                    if (parts.isNotEmpty() && name.startsWith(parts.first()) && parts.drop(1).all { it in name }) {
                        results += PostArea(label, lat, lon)
                        if (results.size == 12) break
                    }
                } else {
                    val town = PostArea(label, lat, lon)
                    val miles = center.milesTo(town)
                    if (miles < distance) { nearest = town; distance = miles }
                }
            }
        }
        return if (center == null) results else listOf(center.copy(label = nearest?.label ?: center.label))
    }

    @Suppress("MissingPermission", "DEPRECATION")
    private suspend fun location(): PostArea {
        val manager = activity.getSystemService(Context.LOCATION_SERVICE) as LocationManager
        val provider = listOf("fused", LocationManager.NETWORK_PROVIDER).firstOrNull { manager.isProviderEnabled(it) }
            ?: error("Location is unavailable. Search for a town instead.")
        val location = withTimeoutOrNull(15000) {
            suspendCancellableCoroutine<android.location.Location?> { continuation ->
                val cancel = CancellationSignal()
                continuation.invokeOnCancellation { cancel.cancel() }
                LocationManagerCompat.getCurrentLocation(manager, provider, cancel, activity.mainExecutor) {
                    if (continuation.isActive) continuation.resume(it)
                }
            }
        } ?: error("Couldn’t find your location. Search for a town instead.")
        return PostArea("Current area", (location.latitude * 100).roundToInt(), (location.longitude * 100).roundToInt())
    }

    override fun request(operation: String, query: String, onResult: (String?, String?) -> Unit) {
        if (operation == "locate" && activity.checkSelfPermission(Manifest.permission.ACCESS_COARSE_LOCATION) != PackageManager.PERMISSION_GRANTED) {
            awaitingPermission = onResult
            permission.launch(Manifest.permission.ACCESS_COARSE_LOCATION)
            return
        }
        if (operation == "search") searchJob?.cancel()
        val job = activity.lifecycleScope.launch {
            try {
                val value = when (operation) {
                    "search" -> withContext(Dispatchers.IO) { Json.encodeToString(towns(query, null)) }
                    "locate" -> { val area = location(); withContext(Dispatchers.IO) { Json.encodeToString(towns(null, area).single()) } }
                    "enroll" -> withContext(Dispatchers.IO) { fetchEnrollment() }
                    else -> error("Unknown location request.")
                }
                onResult(value, null)
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { onResult(null, e.localizedMessage ?: "Couldn’t find this area.") }
        }
        if (operation == "search") searchJob = job
    }
}
