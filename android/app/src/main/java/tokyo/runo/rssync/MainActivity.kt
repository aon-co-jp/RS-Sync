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
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader
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
 * **位置づけ(2026-08-07更新)**: 従来このActivityはRS-Sync本体を
 * Android上で実行しない「リモートクライアント」専用設計だった
 * (ユーザーが入力したリモートURLへ`GET /healthz`で疎通確認するのみ)。
 * ユーザー指示「VPSとスマホの二箇所にインストールしたい」を受け、
 * `open-easy-web/android`と同じ「本体(Rust製バイナリ)を
 * `ProcessBuilder`でこの端末上に直接起動する」方式を追加した
 * (`LocalServerConfig`参照)。**既定は無効のまま**——設定で明示的に
 * 有効化しない限り、従来通りリモートクライアントとして動作する
 * (後方互換)。本体を起動する場合は`RS_SYNC_PRIMARY_HEALTHZ_URL`に
 * VPS側の`/healthz`を設定することで、`src/scheduler.rs`側の
 * フェイルオーバーロジック(2026-08-07実装)によりVPSが生きている間は
 * この端末側が同期ジョブを重複実行しない。
 *
 * 実際のアカウント登録・同期ジョブ作成・実行操作は、本体をこの端末で
 * 起動した場合・リモート接続の場合いずれも「ブラウザで開く」ボタンで
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
    private var localServerProcess: Process? = null

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
        val localServerSettingsButton = findViewById<Button>(R.id.localServerSettingsButton)
        val startLocalServerButton = findViewById<Button>(R.id.startLocalServerButton)

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

        localServerSettingsButton.setOnClickListener {
            showLocalServerSettingsDialog()
        }

        startLocalServerButton.setOnClickListener {
            if (!LocalServerConfig.isEnabled(this)) {
                Toast.makeText(
                    this,
                    "先に「本体を実行する設定」でVPSとの二重稼働を有効化してください",
                    Toast.LENGTH_LONG
                ).show()
                return@setOnClickListener
            }
            startLocalServerButton.isEnabled = false
            CoroutineScope(Dispatchers.Main).launch {
                val log = StringBuilder()
                log.appendLine("profile: ${currentProfile.label} (${currentProfile.prefValue})")
                statusText.text = "[${currentProfile.emoji} ${currentProfile.label}] starting local server..."
                val started = withContext(Dispatchers.IO) { startLocalServerProcess(log) }
                logText.text = log.toString()
                if (!started) {
                    statusText.text = "[${currentProfile.emoji} ${currentProfile.label}] failed to start (see log)"
                    startLocalServerButton.isEnabled = true
                    return@launch
                }
                applyProfilePowerBehavior(log)
                val baseUrl = LocalServerConfig.localBaseUrl(this@MainActivity)
                val healthOk = withContext(Dispatchers.IO) { checkHealth(baseUrl, log) }
                logText.text = log.toString()
                statusText.text = if (healthOk) {
                    "[${currentProfile.emoji} ${currentProfile.label}] RUNNING (local, port ${LocalServerConfig.getLocalPort(this@MainActivity)})"
                } else {
                    "[${currentProfile.emoji} ${currentProfile.label}] started but /healthz did not respond yet (see log)"
                }
                if (healthOk) {
                    serverUrlInput.setText(baseUrl)
                    PowerProfile.saveServerUrl(this@MainActivity, baseUrl)
                    startPeriodicPolling(baseUrl, statusText, logText)
                }
                startLocalServerButton.isEnabled = true
            }
        }

        registerPowerConnectionReceiver()
    }

    /**
     * 有効化・VPS側`/healthz`URL(任意)を設定するダイアログ。
     * `AlertDialog`+動的生成した`EditText`2つの単純なフォーム
     * (open-easy-web/androidの各種設定ダイアログと同じ構成パターン)。
     */
    private fun showLocalServerSettingsDialog() {
        val container = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(48, 24, 48, 0)
        }
        val healthzInput = EditText(this).apply {
            hint = "https://easy-web.tokyo/rs-sync/healthz"
            setText(LocalServerConfig.getPrimaryHealthzUrl(this@MainActivity).orEmpty())
        }
        container.addView(healthzInput)

        AlertDialog.Builder(this)
            .setTitle(R.string.local_server_settings_title)
            .setMessage(R.string.local_server_settings_message)
            .setView(container)
            .setPositiveButton("有効化して保存") { _, _ ->
                LocalServerConfig.setEnabled(this, true)
                LocalServerConfig.setPrimaryHealthzUrl(this, healthzInput.text.toString())
                Toast.makeText(this, "本体実行モードを有効化しました", Toast.LENGTH_SHORT).show()
            }
            .setNeutralButton("無効化に戻す") { _, _ ->
                LocalServerConfig.setEnabled(this, false)
                Toast.makeText(this, "リモートクライアントモードに戻しました", Toast.LENGTH_SHORT).show()
            }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    /**
     * RS-Sync本体(Rust製バイナリ、`cargo ndk`でクロスビルドし
     * `jniLibs/<abi>/librssync.so`として同梱する想定)を`ProcessBuilder`で
     * 起動する(`open-easy-web/android::startServerProcess()`と同じ
     * パターン)。**正直な開示**: このメソッド自体はopen-easy-web版の
     * 確立済み実装パターンに倣って書かれているが、`librssync.so`の
     * 実クロスビルド・実機/エミュレータでの起動確認はこのパスでは
     * 未実施——`binaryPath.exists()`チェックにより、未ビルドの状態で
     * 起動を試みても黙って失敗せず理由をログへ明示する設計にしてある。
     */
    private fun startLocalServerProcess(log: StringBuilder): Boolean {
        return try {
            val binaryPath = File(applicationInfo.nativeLibraryDir, "librssync.so")
            log.appendLine("binary path: ${binaryPath.absolutePath}")
            log.appendLine("binary exists: ${binaryPath.exists()}")
            if (!binaryPath.exists()) {
                log.appendLine(
                    "ERROR: native binary not found — this build was not produced with " +
                        "'cargo ndk ... build --bin rs-sync' output copied into jniLibs/<abi>/librssync.so"
                )
                return false
            }

            val dataDir = File(filesDir, "rs-sync-data")
            dataDir.mkdirs()
            val port = LocalServerConfig.getLocalPort(this)
            val primaryHealthzUrl = LocalServerConfig.getPrimaryHealthzUrl(this)

            val pb = ProcessBuilder(binaryPath.absolutePath)
            pb.directory(filesDir)
            pb.environment()["RS_SYNC_DATA"] = File(dataDir, "store.json").absolutePath
            pb.environment()["RS_SYNC_PORT"] = port.toString()
            // このAndroid端末上のインスタンスへブラウザ以外から到達する
            // 経路が無いため(ループバックのみ)、認証は要求しない——
            // VPS本番とは異なる前提(既存のrs-sync-demo運用と同様の判断)。
            pb.environment()["RS_SYNC_REQUIRE_AUTH"] = "false"
            if (!primaryHealthzUrl.isNullOrBlank()) {
                pb.environment()["RS_SYNC_PRIMARY_HEALTHZ_URL"] = primaryHealthzUrl
                log.appendLine("failover: will defer to primary at $primaryHealthzUrl when reachable")
            } else {
                log.appendLine("failover: no primary healthz URL configured — this device always runs all jobs")
            }
            pb.redirectErrorStream(true)
            val process = pb.start()
            localServerProcess = process

            Thread {
                try {
                    BufferedReader(InputStreamReader(process.inputStream)).use { reader ->
                        var line: String?
                        while (reader.readLine().also { line = it } != null) {
                            android.util.Log.i("rs-sync", line ?: "")
                        }
                    }
                } catch (_: Exception) {
                    // プロセス終了時にストリームが閉じるのは正常系。
                }
            }.start()

            log.appendLine("process started (alive=${process.isAlive}), port=$port")
            true
        } catch (e: Exception) {
            log.appendLine("ERROR launching process: $e")
            false
        }
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
        // Activity終了時に本体プロセスを道連れにする(バックグラウンド
        // 常駐サービス化は今回のスコープ外——open-easy-web版と同様、
        // フォアグラウンドサービス化は次回課題として明記)。
        localServerProcess?.let { if (it.isAlive) it.destroy() }
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
