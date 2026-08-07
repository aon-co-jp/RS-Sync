package tokyo.runo.rssync

import android.content.Context

/**
 * 2026-08-07新設(ユーザー指示「未対応のRS-Syncをスマホにインストール
 * したい」): 従来の`MainActivity`はRS-Sync本体を実行しない「リモート
 * クライアント」専用設計だったが、これを`open-easy-web/android`と同じ
 * 「本体(Rust製バイナリ)をこの端末上で`ProcessBuilder`起動する」方式へ
 * 拡張するための設定値。既定は無効(従来通りリモートクライアントとして
 * 動作)——有効化した場合のみ、`MainActivity`は`libssrssync.so`
 * (`cargo ndk`でクロスビルドしたRS-Sync本体、`jniLibs`配下に同梱)を
 * 起動する。
 *
 * [primaryHealthzUrl]は`RS_SYNC_PRIMARY_HEALTHZ_URL`環境変数として
 * ネイティブプロセスへ渡す——VPS側インスタンスの`/healthz`を指すことで、
 * `scheduler::run_scheduler_loop`のフェイルオーバーロジック
 * (2026-08-07実装、`src/scheduler.rs`参照)がVPS生存中はこの端末側の
 * ジョブ実行を自動的にスキップする(重複同期回避)。
 */
object LocalServerConfig {
    private const val PREFS_NAME = "open_redmine_prefs"
    private const val KEY_LOCAL_SERVER_ENABLED = "local_server_enabled"
    private const val KEY_PRIMARY_HEALTHZ_URL = "primary_healthz_url"
    private const val KEY_LOCAL_PORT = "local_server_port"

    const val DEFAULT_PORT = 18095

    fun isEnabled(context: Context): Boolean {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        return prefs.getBoolean(KEY_LOCAL_SERVER_ENABLED, false)
    }

    fun setEnabled(context: Context, enabled: Boolean) {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        prefs.edit().putBoolean(KEY_LOCAL_SERVER_ENABLED, enabled).apply()
    }

    /** VPS側`/healthz`のフルURL(例: `https://easy-web.tokyo/rs-sync/healthz`)。未設定ならフェイルオーバーを行わない(常にこの端末が実行する)。 */
    fun getPrimaryHealthzUrl(context: Context): String? {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        return prefs.getString(KEY_PRIMARY_HEALTHZ_URL, null)?.takeIf { it.isNotBlank() }
    }

    fun setPrimaryHealthzUrl(context: Context, url: String?) {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        prefs.edit().putString(KEY_PRIMARY_HEALTHZ_URL, url?.trim().orEmpty()).apply()
    }

    fun getLocalPort(context: Context): Int {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        return prefs.getInt(KEY_LOCAL_PORT, DEFAULT_PORT)
    }

    fun setLocalPort(context: Context, port: Int) {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        prefs.edit().putInt(KEY_LOCAL_PORT, port).apply()
    }

    /** 本体プロセスが使うローカルURL(このActivity自身の「ブラウザで開く」・ヘルスチェック疎通確認の両方で使う)。 */
    fun localBaseUrl(context: Context): String = "http://127.0.0.1:${getLocalPort(context)}"
}
