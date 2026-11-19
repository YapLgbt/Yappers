package app.yappers

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class GroupsTest {
    private val ny = GroupLocality("New York", "NY", "US")
    private val london = GroupLocality("London", "", "GB")
    private fun group(id: String, name: String, city: GroupLocality?) =
        GroupSummary(id, "owner", name, "A group for readers", "Friends", city)
    private val catalog = listOf(group("ny", "City readers", ny), group("ldn", "London readers", london), group("world", "Readers everywhere", null))

    @Test fun localAndWorldwideScopesStaySeparateAndCityMatchingIsCaseInsensitive() {
        assertEquals(listOf("ny"), visibleGroups(catalog, false, GroupLocality(" new york ", "ny", "US")).map { it.id })
        assertEquals(listOf("world"), visibleGroups(catalog, true, ny).map { it.id })
        assertEquals(2, visibleGroups(catalog, false, null).size)
        assertTrue(visibleGroups(catalog, false, ny, "London").isEmpty())
    }

    @Test fun sameNamedCitiesInDifferentRegionsOrCountriesDoNotMerge() {
        assertTrue(!ny.samePlace(GroupLocality("New York", "NY", "GB")))
        assertTrue(!ny.samePlace(GroupLocality("New York", "NJ", "US")))
        assertEquals(listOf("ldn"), visibleGroups(catalog, false, null, "london").map { it.id })
    }
}
