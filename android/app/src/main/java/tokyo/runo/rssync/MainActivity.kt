package tokyo.runo.rssync

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.os.Bundle
import android.os.PowerManager
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * RS-Sync Android版クライアント(参照実装`open-web-server/android/`・
 * `aruaru-db/android/`と同じ電源プロファイル・BroadcastReceiverパターンを
 * 踏襲)。
 *
 * **重要な位置づけ**: このActivityはRS-Sync本体(Rust製のGitホスティング
 * 間ミラー同期・フォルダ同期サーバー、日英2言語WebUI)をAndroid上で
 * 実行するものではない。ユーザーが入力したリモートURL(既存のRS-Sync
 * インスタンス、`GET /healthz`)へHTTPで接続し疎通確認を行い、実際の
 * アカウント登録・同期ジョブ作成・実行操作は「ブラウザで開く」ボタンで
 * 外部ブラウザ(既存のWebUI)に委ねる(open-redmine/open-easy-web版と
 * 同じ「ネイティブUIを再実装しない」設計判断)。
 *
 * スコープ(意図的に含めない): 同期ジョブ一覧・作成等のネイティブUI
 * 再実装、プッシュ通知、埋め込みWebView(外部ブラウザへの遷移のみ)。
 */
class MainActivity : AppCompatActivity() {

    companion object {
        const val EXTRA_PROFILE = "profile"
    }

    /**
     * プロファイル別の疎通確認ポーリング間隔(open-web-server/android版の
     * `healthPollIntervalMs`と同じ考え方)。省電力版は間隔を大きく延ばし
     * (Doze/App Standbyへの影響を最小化)、常時電源接続版は短い間隔で
     * 即応性を優先する。
     */
    private fun pollIntervalMs(profile: PowerProfile): Long = when (profile) {
        PowerProfile.POWER_SAVE -> 5 * 60_000L // 5分
        PowerProfile.NORMAL -> 60_000L // 1分
        PowerProfile.ALWAYS_ON -> 5_000L // 5秒
    }

    private var wakeLock: PowerManager.WakeLock? = null
    private var pollJob: Job? = null
    private var powerConnectionReceiver: BroadcastReceiver? = null
    private lateinit var currentProfile: PowerProfile

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        currentProfile = resolveProfile()
        PowerProfile.save(this, currentProfile)

        val statusText = findViewById<TextView>(R.id.statusText)
        val logText = findViewById<TextView>(R.id.logText)
        val serverUrlInput = findViewById<EditText>(R.id.serverUrlInput)
        val connectButton = findViewById<Button>(R.id.connectButton)
        val openBrowserButton = findViewById<Button>(R.id.openBrowserButton)
        val changeProfileButton = findViewById<Button>(R.id.changeProfileButton)

        serverUrlInput.setText(PowerProfile.loadServerUrl(this))
        statusText.text = "RS-Sync [${currentProfile.emoji} ${currentProfile.label}モード] (未接続)"

        connectButton.setOnClickListener {
            val url = serverUrlInput.text.toString().trim().trimEnd('/')
            if (url.isEmpty()) {
                Toast.makeText(this, "接続先URLを入力してください", Toast.LENGTH_SHORT).show()
                return@setOnClickListener
            }
            PowerProfile.saveServerUrl(this, url)
            connectButton.isEnabled = false
            CoroutineScope(Dispatchers.Main).launch {
                statusText.text = "[${currentProfile.emoji} ${currentProfile.label}] 接続確認中..."
                val log = StringBuilder()
                val ok = withContext(Dispatchers.IO) { checkHealth(url, log) }
                statusText.text = if (ok) {
                    "[${currentProfile.emoji} ${currentProfile.label}] 接続OK"
                } else {
                    "[${currentProfile.emoji} ${currentProfile.label}] 接続失敗(ログ参照)"
                }
                logText.text = log.toString()
                connectButton.isEnabled = true
                if (ok) {
                    applyProfilePowerBehavior(log)
                    logText.text = log.toString()
                    startPeriodicPolling(url, statusText, logText)
                }
            }
        }

        openBrowserButton.setOnClickListener {
            val url = serverUrlInput.text.toString().trim().trimEnd('/')
            if (url.isEmpty()) {
                Toast.makeText(this, "接続先URLを入力してください", Toast.LENGTH_SHORT).show()
                return@setOnClickListener
            }
            PowerProfile.saveServerUrl(this, url)
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("$url/")))
        }

        changeProfileButton.setOnClickListener {
            startActivity(Intent(this, ProfileSelectActivity::class.java))
            finish()
        }

        registerPowerConnectionReceiver()
    }

    private fun resolveProfile(): PowerProfile {
        return when (intent?.action) {
            "tokyo.runo.rssync.LAUNCH_POWER_SAVE" -> PowerProfile.POWER_SAVE
            "tokyo.runo.rssync.LAUNCH_NORMAL" -> PowerProfile.NORMAL
            "tokyo.runo.rssync.LAUNCH_ALWAYS_ON" -> PowerProfile.ALWAYS_ON
            else -> {
                val extra = intent?.getStringExtra(EXTRA_PROFILE)
                if (extra != null) PowerProfile.fromPrefValue(extra) else PowerProfile.load(this)
            }
        }
    }

    /**
     * プロファイルごとの電源管理の中身(open-web-server/android版の
     * `applyProfilePowerBehavior`と同じ設計)。省電力/通常はWakeLockを
     * 取得しない、常時電源接続のみ`PARTIAL_WAKE_LOCK`を保持する。
     */
    private fun applyProfilePowerBehavior(log: StringBuilder) {
        when (currentProfile) {
            PowerProfile.ALWAYS_ON -> {
                try {
                    val pm = getSystemService(POWER_SERVICE) as PowerManager
                    val lock = pm.newWakeLock(
                        PowerManager.PARTIAL_WAKE_LOCK,
                        "RsSyncMonitor::AlwaysOnWakeLock"
                    )
                    lock.acquire()
                    wakeLock = lock
                    log.appendLine("power: acquired PARTIAL_WAKE_LOCK (always-on profile)")
                } catch (e: Exception) {
                    log.appendLine("power: failed to acquire WakeLock: ${e.message}")
                }
            }
            PowerProfile.POWER_SAVE -> {
                log.appendLine("power: no WakeLock acquired (power-save profile, Doze-friendly)")
            }
            PowerProfile.NORMAL -> {
                log.appendLine("power: no WakeLock acquired (normal profile)")
            }
        }
    }

    /**
     * 電源の抜き差し監視(open-web-server/android版と同じダイアログ導線)。
     * 常時電源接続版実行中に電源が外れたら省電力/通常への切替を尋ね、
     * 逆に電源が再接続されたら常時電源接続への切替を尋ねる。
     */
    private fun registerPowerConnectionReceiver() {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                when (intent.action) {
                    Intent.ACTION_POWER_DISCONNECTED -> onPowerDisconnected()
                    Intent.ACTION_POWER_CONNECTED -> onPowerConnected()
                }
            }
        }
        powerConnectionReceiver = receiver
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_POWER_DISCONNECTED)
            addAction(Intent.ACTION_POWER_CONNECTED)
        }
        registerReceiver(receiver, filter)
    }

    private fun onPowerDisconnected() {
        if (currentProfile != PowerProfile.ALWAYS_ON) return
        if (isFinishing || isDestroyed) return
        AlertDialog.Builder(this)
            .setTitle("電源が外れました")
            .setMessage(
                "常時電源接続モードで監視中に電源が外れました。\n" +
                    "省電力モードに切り替えますか?それとも通常モードの" +
                    "ままにしますか?\n(推奨: 省電力モード)"
            )
            .setPositiveButton("省電力モードへ切替") { _, _ ->
                switchProfileAndRestart(PowerProfile.POWER_SAVE)
            }
            .setNegativeButton("通常モードのままにする") { _, _ ->
                switchProfileAndRestart(PowerProfile.NORMAL)
            }
            .setCancelable(false)
            .show()
    }

    private fun onPowerConnected() {
        if (currentProfile == PowerProfile.ALWAYS_ON) return
        if (isFinishing || isDestroyed) return
        AlertDialog.Builder(this)
            .setTitle("電源が接続されました")
            .setMessage("常時電源接続モード(短間隔監視)に切り替えますか?")
            .setPositiveButton("常時電源接続へ切替") { _, _ ->
                switchProfileAndRestart(PowerProfile.ALWAYS_ON)
            }
            .setNegativeButton("このままにする", null)
            .show()
    }

    private fun switchProfileAndRestart(newProfile: PowerProfile) {
        PowerProfile.save(this, newProfile)
        Toast.makeText(
            this,
            "${newProfile.emoji} ${newProfile.label}モードへ切り替えます",
            Toast.LENGTH_SHORT
        ).show()
        val intent = Intent(this, MainActivity::class.java)
        intent.putExtra(EXTRA_PROFILE, newProfile.prefValue)
        startActivity(intent)
        finish()
    }

    /**
     * RS-Syncの`GET /healthz`(`src/main.rs::healthz`、平文`"ok"`を
     * 返す)へ接続する。JSON構造は持たないため、本文をそのままログへ表示する
     * だけの単純な疎通確認(aruaru-db版のようなクラスタ統計JSONパースは
     * RS-Syncには存在しないため対象外)。
     */
    private fun checkHealth(baseUrl: String, log: StringBuilder): Boolean {
        return try {
            val url = URL("$baseUrl/healthz")
            val conn = url.openConnection() as HttpURLConnection
            conn.connectTimeout = 5000
            conn.readTimeout = 5000
            conn.requestMethod = "GET"
            val code = conn.responseCode
            log.appendLine("GET $url -> $code")
            if (code == 200) {
                val body = conn.inputStream.bufferedReader().readText()
                log.appendLine("body: $body")
                conn.disconnect()
                true
            } else {
                conn.disconnect()
                false
            }
        } catch (e: Exception) {
            log.appendLine("ERROR: ${e.message}")
            false
        }
    }

    /**
     * 継続的な疎通監視ループ(open-web-server/android版の
     * `startPeriodicHealthPoll`と同じ設計)。プロファイルごとに間隔を
     * 変えることが「省電力版が実際に省電力になる」施策そのもの。
     */
    private fun startPeriodicPolling(baseUrl: String, statusText: TextView, logText: TextView) {
        pollJob?.cancel()
        val intervalMs = pollIntervalMs(currentProfile)
        pollJob = CoroutineScope(Dispatchers.Main).launch {
            while (isActive) {
                delay(intervalMs)
                val log = StringBuilder()
                val ok = withContext(Dispatchers.IO) { checkHealth(baseUrl, log) }
                statusText.text = if (ok) {
                    "[${currentProfile.emoji} ${currentProfile.label}] 監視中 " +
                        "(${intervalMs / 1000}秒間隔)"
                } else {
                    "[${currentProfile.emoji} ${currentProfile.label}] 接続失敗"
                }
                logText.text = log.toString()
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        pollJob?.cancel()
        powerConnectionReceiver?.let {
            try {
                unregisterReceiver(it)
            } catch (_: IllegalArgumentException) {
                // 未登録のまま呼ばれても(onCreateの早期return等)無視する。
            }
        }
        if (wakeLock?.isHeld == true) {
            wakeLock?.release()
        }
    }
}
