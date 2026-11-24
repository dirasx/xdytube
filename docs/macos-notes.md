# Технические заметки xdytube для macOS

Целевая платформа — Apple Silicon. Приложение написано на Rust, интерфейс использует GPUI, воспроизведение — libmpv. Задачи ведутся в [планах](../plans/next-release.md).

## Safari и YouTube

`YouTube account → Sign in` выдаёт код устройства. Страницу подтверждения можно открыть в Safari как в системном браузере. Такой вход обслуживает функции аккаунта, в том числе чтение истории и Watch Later. [Вход по коду](../src/api/auth.rs)

`YouTube watch history → Connect` использует отдельный временный профиль Chromium-браузера для подключения отправки просмотров и позиции из libmpv. Системная `ASWebAuthenticationSession` может вернуть результат OAuth. Сессия обычного профиля Safari этим способом недоступна. `WKWebView` использует собственное хранилище, а Safari WebDriver работает с изолированным окном. У публичного YouTube Data API нет метода записи просмотров и позиции. [Текущий механизм](../src/api/web.rs) · [Apple AuthenticationServices](https://developer.apple.com/documentation/authenticationservices/supporting-single-sign-on-in-a-web-browser-app) · [YouTube Data API](https://developers.google.com/youtube/data/revision_history)

Для просмотра в Safari можно отдельно рассмотреть открытие видео с текущей позицией. Сценарий подключения истории из xdytube остаётся в [ДЯ](../plans/backlog.md#доработать-подключение-истории-youtube).
