package app.yappers

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ShortNavigationBarItemDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteItem
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.ui.Modifier
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.ui.NavDisplay
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
import app.yappers.shared.R

internal object AndroidNavigation : MaterialNavigation {
    private val icons = mapOf(
        "Nearby" to (R.drawable.nav_location_on to R.drawable.nav_location_on_selected),
        "Groups" to (R.drawable.nav_groups to R.drawable.nav_groups_selected),
        "Saved" to (R.drawable.nav_bookmark to R.drawable.nav_bookmark_selected),
        "You" to (R.drawable.nav_person to R.drawable.nav_person_selected)
    )

    @Composable
    override fun Pages(stack: List<MaterialPage>, onBack: () -> Unit, content: @Composable (MaterialPage) -> Unit) {
        val motion = MaterialTheme.motionScheme
        NavDisplay(
            backStack = stack,
            onBack = onBack,
            modifier = Modifier.fillMaxSize(),
            transitionSpec = {
                (fadeIn(motion.defaultEffectsSpec()) + scaleIn(motion.defaultSpatialSpec(), initialScale = .94f))
                    .togetherWith(fadeOut(motion.fastEffectsSpec()))
            },
            popTransitionSpec = {
                fadeIn(motion.defaultEffectsSpec()).togetherWith(
                    fadeOut(motion.fastEffectsSpec()) + scaleOut(motion.defaultSpatialSpec(), targetScale = .94f)
                ).apply { targetContentZIndex = -1f }
            },
            entryProvider = { page -> NavEntry(page) { content(it) } }
        )
    }

    @Composable
    override fun Step(selected: String, content: @Composable (String) -> Unit) {
        val motion = MaterialTheme.motionScheme
        AnimatedContent(selected, modifier = Modifier.fillMaxSize(), transitionSpec = {
            fadeIn(motion.defaultEffectsSpec()) togetherWith fadeOut(motion.fastEffectsSpec())
        }, label = "Change section") { content(it) }
    }

    @Composable
    override fun Layout(selected: String, onSelect: (String) -> Unit, content: @Composable (String) -> Unit) {
        val scheme = MaterialTheme.colorScheme
        val colors = ShortNavigationBarItemDefaults.colors(
            selectedIconColor = scheme.onPrimary,
            selectedTextColorTopIconPosition = scheme.primary,
            selectedTextColorStartIconPosition = scheme.onPrimary,
            selectedIndicatorColor = scheme.primary
        )
        NavigationSuiteScaffold(navigationItems = {
            destinations.forEach { (label, _) ->
                val active = label == selected
                val (outline, filled) = icons.getValue(label)
                NavigationSuiteItem(
                    selected = active,
                    onClick = { onSelect(label) },
                    icon = { Icon(painterResource(if (active) filled else outline), contentDescription = null) },
                    label = { Text(label) },
                    colors = colors
                )
            }
        }) {
            val holder = rememberSaveableStateHolder()
            Step(selected) { tab ->
                holder.SaveableStateProvider(tab) { content(tab) }
            }
        }
    }

    @OptIn(ExperimentalMaterial3Api::class)
    @Composable
    override fun UpBar(title: String, onUp: () -> Unit, actions: @Composable RowScope.() -> Unit) {
        TopAppBar(
            title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            navigationIcon = {
                IconButton(onClick = onUp) {
                    Icon(painterResource(R.drawable.ic_arrow_back), contentDescription = "Navigate up")
                }
            },
            actions = actions,
            // The outer Scaffold consumes the system bar insets.
            windowInsets = WindowInsets(0)
        )
    }
}
