package app.yappers

import android.app.Application
import android.content.Context
import androidx.work.*
import kotlinx.coroutines.*
import java.io.IOException
import java.net.URL
import java.util.concurrent.TimeUnit
import javax.net.ssl.HttpsURLConnection

internal fun fetchEnrollment(): String {
    val conn = URL("https://yap.lgbt/.well-known/yap-home.json").openConnection() as HttpsURLConnection
    try {
        conn.connectTimeout = 8000
        conn.readTimeout = 8000
        conn.instanceFollowRedirects = false
        check(conn.responseCode == 200) { "Couldn’t reach yap.lgbt." }
        return conn.inputStream.bufferedReader().use { reader ->
            val out = StringBuilder()
            val buf = CharArray(2048)
            while (true) {
                val n = reader.read(buf)
                if (n < 0) break
                require(out.length + n <= 16384) { "Couldn’t read the home invitation. Try again later." }
                out.append(buf, 0, n)
            }
            out.toString()
        }
    } catch (failure: IOException) {
        throw IllegalStateException("Couldn’t reach yap.lgbt. Check your connection and try again.", failure)
    } finally { conn.disconnect() }
}

class YapApplication : Application() {
    internal val storage by lazy { AndroidStorage(this) }
    internal val controller by lazy {
        NetkitController(storage, AndroidNetkitEngine()) {
            withContext(kotlinx.coroutines.Dispatchers.IO) { fetchEnrollment() }
        }.also { it.setForeground(false); it.prepareWelcome(storage.local.read() != null) }
    }
    private val background = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    internal fun leaveForeground() {
        controller.setForeground(false)
        background.launch { controller.closeIfBackground() }
        val connection = controller.state.value
        if (connection.enabled) scheduleSync(connection.pendingCount > 0)
    }
    override fun onCreate() {
        super.onCreate()
        AndroidAbi.initialize(this)
    }
    internal fun scheduleSync(pending: Boolean) {
        val constraints = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
        val work = WorkManager.getInstance(this)
        work.enqueueUniquePeriodicWork("yap-refresh", ExistingPeriodicWorkPolicy.KEEP,
            PeriodicWorkRequestBuilder<YapWorker>(15, TimeUnit.MINUTES).setConstraints(constraints).build())
        if (pending) work.enqueueUniqueWork("yap-delivery", ExistingWorkPolicy.KEEP,
            OneTimeWorkRequestBuilder<YapWorker>().setConstraints(constraints).build())
    }
}

class YapWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = coroutineScope {
        val app = applicationContext as YapApplication
        val token = AndroidAbi.beginRefresh()
        val monitor = launch(start = CoroutineStart.UNDISPATCHED) {
            try { awaitCancellation() } finally { AndroidAbi.endRefresh(token) }
        }
        try {
            val completed = withContext(AndroidRefreshToken(token)) {
                app.controller.backgroundRefresh(FeedRepository(app.storage.local).snapshot.nearby)
            }
            if (completed == false) Result.retry() else Result.success()
        } catch (e: kotlinx.coroutines.CancellationException) { throw e }
        catch (_: Exception) { Result.retry() }
        finally { monitor.cancel() }
    }
}
