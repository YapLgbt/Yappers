package app.yappers

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.pullToRefresh
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

internal val LocalRefreshIndicator = staticCompositionLocalOf<(@Composable (Float, Boolean) -> Unit)?> { null }
internal val LocalTransientNotice = staticCompositionLocalOf<((String) -> Unit)?> { null }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun FeedRefreshPane(modifier: Modifier, enabled: Boolean, refreshing: Boolean,
                             onRefresh: () -> Unit, content: @Composable () -> Unit) {
    val indicator = LocalRefreshIndicator.current
    if (indicator == null) {
        val pull = rememberPullToRefreshState()
        Box(modifier.pullToRefresh(refreshing, pull, enabled = enabled, onRefresh = onRefresh), propagateMinConstraints = true) {
            content()
            if (enabled) PullToRefreshDefaults.Indicator(
                modifier = Modifier.align(Alignment.TopCenter), isRefreshing = refreshing, state = pull)
        }
        return
    }
    val threshold = with(LocalDensity.current) { 96.dp.toPx() }
    val reducedMotion = platformReducesMotion()
    val refresh by rememberUpdatedState(onRefresh)
    val busy by rememberUpdatedState(refreshing)
    val connection = remember(enabled, threshold) {
        FeedPullConnection(enabled, threshold, isRefreshing = { busy }, onRefresh = { refresh() })
    }
    val drag = connection.drag
    val offset by animateFloatAsState(
        targetValue = if (!enabled) 0f else if (refreshing) threshold / 2 else drag / 2,
        animationSpec = if (drag > 0f || reducedMotion) snap() else tween(180),
        label = "Refresh inset",
    )
    Box(modifier.clipToBounds().nestedScroll(connection), propagateMinConstraints = true) {
        Box(Modifier.fillMaxSize().offset { IntOffset(0, offset.roundToInt()) }, propagateMinConstraints = true) { content() }
        if (offset > 1f) {
            Box(Modifier.align(Alignment.TopCenter).wrapContentSize(Alignment.TopCenter)
                .height(with(LocalDensity.current) { offset.toDp() }).clipToBounds(), contentAlignment = Alignment.Center) {
                indicator((drag / threshold).coerceIn(0f, 1f), refreshing)
            }
        }
    }
}

// Consume only excess downward finger movement at the top of the list. A fling
// reaching the top never arms a refresh, and reversing the drag can disarm it.
internal class FeedPullConnection(
    private val enabled: Boolean,
    private val threshold: Float,
    private val isRefreshing: () -> Boolean,
    private val onRefresh: () -> Unit,
) : NestedScrollConnection {
    var drag by mutableFloatStateOf(0f)
        private set

    override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
        if (source != NestedScrollSource.UserInput || drag == 0f || available.y >= 0f) return Offset.Zero
        val consumed = available.y.coerceAtLeast(-drag)
        drag += consumed
        return Offset(0f, consumed)
    }

    override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
        if (!enabled || isRefreshing() || source != NestedScrollSource.UserInput || available.y <= 0f) return Offset.Zero
        drag = (drag + available.y).coerceAtMost(threshold * 1.5f)
        return Offset(0f, available.y)
    }

    override suspend fun onPreFling(available: Velocity): Velocity {
        if (drag == 0f) return Velocity.Zero
        val armed = enabled && !isRefreshing() && drag >= threshold
        drag = 0f
        if (armed) onRefresh()
        return Velocity(0f, available.y.coerceAtLeast(0f))
    }
}
