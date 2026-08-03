// RS-Sync Android版: リモートのRS-Syncインスタンス(ブラウザGUI)への
// 接続確認+起動導線を提供するクライアントアプリ。open-web-server/android/・
// aruaru-db/android/(参照実装)と同じGradle構成パターンに従う。
//
// 位置づけ(重要): このアプリはRS-Sync本体(Rust製Gitミラー同期/フォルダ同期サーバー)を
// Android上で動かすものではない。稼働中のRS-Syncインスタンスへ`/healthz`で
// 疎通確認を行い、実際のアカウント登録・同期ジョブ管理は既存の
// ブラウザGUI(WASM)を外部ブラウザで開く形に委ねる(open-easy-web/android版と
// 同じ「ネイティブUIを再実装しない」設計判断)。
plugins {
    id("com.android.application") version "8.7.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
