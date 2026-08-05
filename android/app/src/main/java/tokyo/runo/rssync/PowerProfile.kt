package tokyo.runo.rssync

import android.content.Context

/**
 * 3電源プロファイル(open-web-server/android版・aruaru-db/android版と
 * 同じパターンを踏襲)。
 *
 * このアプリはRS-Sync本体を実行するのではなく、リモートの
 * インスタンスへ`/healthz`で疎通確認するクライアントであるため、
 * プロファイルごとの実際の差は「疎通確認ポーリング間隔」と
 * 「WakeLockの有無」になる。
 *
 * - [POWER_SAVE] 省電力版: ポーリング間隔を長くし、`WakeLock`を取得しない
 *   (Android Doze/App Standbyに逆らわない)。
 * - [NORMAL] 通常版: 上記2つの中間。バランス型(既定値)。
 * - [ALWAYS_ON] 常時電源接続版: 充電器に繋ぎっぱなしの監視専用端末向け。
 *   `PARTIAL_WAKE_LOCK`を保持し、短間隔で監視し続ける。
 */
enum class PowerProfile(val prefValue: String, val label: String, val emoji: String) {
    POWER_SAVE("power_save", "省電力", "🔋⚡️✕"),
    NORMAL("normal", "通常", "⚖️"),
    ALWAYS_ON("always_on", "常時電源接続", "🔌");

    companion object {
        private const val PREFS_NAME = "open_redmine_prefs"
        private const val KEY_PROFILE = "power_profile"
        private const val KEY_SERVER_URL = "server_url"

        fun fromPrefValue(value: String?): PowerProfile =
            values().firstOrNull { it.prefValue == value } ?: NORMAL

        fun load(context: Context): PowerProfile {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            return fromPrefValue(prefs.getString(KEY_PROFILE, null))
        }

        fun save(context: Context, profile: PowerProfile) {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            prefs.edit().putString(KEY_PROFILE, profile.prefValue).apply()
        }

        fun loadServerUrl(context: Context): String {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            // 2026-08-05修正: 実機検証で発覚した実バグ——ここが誤って
            // "RS-Sync"(大文字)になっており、実際の本番パス
            // "https://easy-web.tokyo/rs-sync/"(小文字、nginxの
            // location設定は大文字小文字を区別する)と一致していなかった。
            // strings.xmlのserver_url_hintは元々正しく小文字だったため、
            // 初回起動時にヒント表示と実際のデフォルト値が食い違って
            // いた状態。
            return prefs.getString(KEY_SERVER_URL, null) ?: "https://easy-web.tokyo/rs-sync/"
        }

        fun saveServerUrl(context: Context, url: String) {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            prefs.edit().putString(KEY_SERVER_URL, url).apply()
        }
    }
}
