package app.yappers

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/** Manually selected city metadata. No coordinates, GPS permission, or residence claim. */
@Serializable
data class GroupLocality(val city: String, val region: String = "", val country: String) {
    val label: String get() = listOf(city, region, country).filter { it.isNotBlank() }.joinToString(", ")
    fun samePlace(other: GroupLocality): Boolean = country == other.country &&
        city.trim().lowercase() == other.city.trim().lowercase() && region.trim().lowercase() == other.region.trim().lowercase()
}

@Serializable
data class GroupSummary(
    val id: String, val owner: String, val name: String, val description: String,
    val category: String, val locality: GroupLocality? = null, val homes: Int = 0, val hosted: Boolean = false
)

@Serializable
internal data class SavedGroup(val summary: GroupSummary, val join: JsonObject)

/** Separate Keychain record; group capabilities never enter the public catalog or UserDefaults. */
@Serializable
internal data class GroupPreferences(
    val version: Int = 1,
    val joined: Map<String, SavedGroup> = emptyMap(),
    val locality: GroupLocality? = null,
    val home: JsonObject? = null
)

/** Scope filtering happens on-device; browsing another city does not transmit that choice. */
fun visibleGroups(
    groups: List<GroupSummary>, worldwide: Boolean, locality: GroupLocality?, query: String = ""
): List<GroupSummary> {
    val needle = query.trim().lowercase()
    return groups.filter { group ->
        val scope = if (worldwide) group.locality == null else group.locality != null &&
            (locality == null || group.locality.samePlace(locality))
        scope && (needle.isEmpty() || "${group.name} ${group.description} ${group.category}".lowercase().contains(needle))
    }.sortedWith(compareBy<GroupSummary> { it.name.lowercase() }.thenBy { it.id })
}
