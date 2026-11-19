package app.yappers

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.unit.Velocity
import kotlinx.coroutines.runBlocking
import kotlin.test.Test
import kotlin.test.assertEquals

class FeedPullTest {
    @Test fun refreshNeedsAnUnconsumedFingerDragAndFiresOnceOnRelease() = runBlocking {
        var refreshes = 0
        val pull = FeedPullConnection(true, 96f, { false }, { refreshes++ })
        val down = Offset(0f, 96f)
        pull.onPostScroll(down, Offset.Zero, NestedScrollSource.UserInput)
        pull.onPostScroll(Offset.Zero, down, NestedScrollSource.SideEffect)
        assertEquals(0f, pull.drag)
        pull.onPostScroll(Offset.Zero, down, NestedScrollSource.UserInput)
        assertEquals(0, refreshes)
        val velocity = Velocity(2f, 30f)
        assertEquals(Velocity(0f, 30f), pull.onPreFling(velocity))
        assertEquals(Velocity.Zero, pull.onPreFling(velocity))
        assertEquals(1, refreshes)
        assertEquals(0f, pull.drag)
    }

    @Test fun reversingDirectionOrStartingAnotherRefreshCancelsThePull() = runBlocking {
        var busy = false
        var refreshes = 0
        val pull = FeedPullConnection(true, 96f, { busy }, { refreshes++ })
        val down = Offset(0f, 120f)
        pull.onPostScroll(Offset.Zero, down, NestedScrollSource.UserInput)
        assertEquals(Offset(0f, -30f), pull.onPreScroll(Offset(2f, -30f), NestedScrollSource.UserInput))
        pull.onPreFling(Velocity.Zero)
        pull.onPostScroll(Offset.Zero, down, NestedScrollSource.UserInput)
        busy = true
        pull.onPreFling(Velocity.Zero)
        assertEquals(Offset.Zero, pull.onPostScroll(Offset.Zero, down, NestedScrollSource.UserInput))
        val disabled = FeedPullConnection(false, 96f, { false }, { refreshes++ })
        assertEquals(Offset.Zero, disabled.onPostScroll(Offset.Zero, down, NestedScrollSource.UserInput))
        disabled.onPreFling(Velocity.Zero)
        assertEquals(0, refreshes)
    }
}
