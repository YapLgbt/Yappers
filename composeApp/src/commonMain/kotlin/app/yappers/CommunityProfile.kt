package app.yappers

import androidx.compose.ui.graphics.ImageBitmap
import kotlinx.serialization.Serializable

internal const val PROFILE_PHOTO_SIZE = 1024
internal const val PROFILE_PHOTO_LENGTH = 384 * 1024
internal const val PROFILE_THUMBNAIL_SIZE = 384
internal const val PROFILE_THUMBNAIL_LENGTH = 24 * 1024
internal const val LEGACY_PROFILE_PHOTO_LENGTH = 4096

/** Optional self-description. Never inferred, required, or attached to anonymous posts. */
@Serializable
data class CommunityProfile(
    val pronouns: String = "",
    val bio: String = "",
    val identity: String = "",
    val interests: String = "",
    val hereFor: List<String> = emptyList(),
    val photo: String = ""
) {
    fun cleaned(): CommunityProfile {
        require(pronouns.length <= 40) { "Keep pronouns to 40 characters." }
        require(bio.length <= 280) { "Keep your bio to 280 characters." }
        require(identity.length <= 80 && interests.length <= 120) { "Shorten your identity or interests." }
        require(hereFor.size <= 6 && hereFor.all { it in profileIntentions }) { "Choose up to six intentions." }
        require(photo.length <= PROFILE_PHOTO_LENGTH && photo.all { it.isLetterOrDigit() && it.code < 128 || it in "+/=" }) { "Choose a smaller profile photo." }
        return copy(pronouns = pronouns.trim(), bio = bio.trim(), identity = identity.trim(),
            interests = interests.trim(), hereFor = hereFor.distinct())
    }
}

val profileIntentions = listOf("Friends", "Dates", "Community", "Creative pals", "Chosen family", "Just yapping")

/** Returns an uncropped local preview (longest edge at most 1536px). Cancellation returns two nulls. */
interface ProfilePhotoLibrary {
    fun choosePhoto(onResult: (String?, String?) -> Unit)
}

/** Decode only bounded thumbnails. Invalid remote image data falls back to an author glyph. */
expect fun decodeProfilePhoto(value: String): ImageBitmap?
