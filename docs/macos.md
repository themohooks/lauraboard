# macOS

Для Intel x86-64 и Apple Silicon ARM64 предусмотрены отдельные нативные сборки для macOS 15 и новее. Релизный workflow собирает GUI, запускает тесты на соответствующем Mac runner и формирует `.app` в portable-архиве, DMG и серверный архив.

CI проверяет desktop и server отдельно на Intel и Apple Silicon. Workflow `Build macOS` можно запустить вручную для обеих архитектур или одной выбранной; он проверяет GUI, HTML/SWF-захват и публикует `.app`-архив, DMG и серверный архив как артефакты запуска. Релиз использует тот же workflow.

Откройте DMG и перенесите LauraBoard в «Программы». Приложение содержит FFmpeg/ffprobe с перемещаемыми библиотеками, небольшой WebView-компонент на системном WKWebView и автономный Ruffle. Для локальных CLI-команд можно использовать `/Applications/LauraBoard.app/Contents/MacOS/lauraboard`. Проекты и настройки хранятся вне `.app` и сохраняются при обновлении. Обновления из интерфейса скачивают подходящий DMG с проверкой SHA256.

Поддерживаются Retina, системные файловые диалоги, Cmd-сочетания, значок в строке меню и аппаратный H.264 через VideoToolbox, если пробное кодирование FFmpeg проходит. Автовыбор кодека сохраняет программный fallback.

Для запуска сервера после входа в учётную запись:

```sh
lauraboard --project /path/to/project service install --name lauraboard
lauraboard service uninstall --name lauraboard
```

Используется пользовательский LaunchAgent в `~/Library/LaunchAgents`, не systemd. До входа пользователя в систему такой агент не запускается.

Для сборки из исходников нужны Rust, Xcode Command Line Tools, Python 3.11+ и нативный FFmpeg (`brew install ffmpeg`). `RELEASE_TARGET` должен совпадать с `rustc -vV`. Команда упаковки: `python3 .github/scripts/release.py build`; результат — `target/release-assets`.

По умолчанию применяется ad-hoc подпись с проверкой структуры bundle. Для доверенной публичной поставки необходимы сертификат Developer ID и отдельная notarization; учётные данные Apple в репозитории не хранятся. Переменная `MACOS_SIGNING_IDENTITY` позволяет выбрать установленный сертификат. Нативный запуск на Mac должен подтверждаться macOS CI: Windows-проверка не заменяет этот этап.

Размещение вложенного кода и порядок подписи соответствуют рекомендациям [Apple Code Signing](https://developer.apple.com/library/archive/technotes/tn2206/): отдельные бинарники подписываются до внешнего bundle; `--deep` применяется только для проверки.

Для постоянного качества VideoToolbox используется `q:v`, а не CRF. На Intel этот режим переводится на libx264, поскольку [реализация FFmpeg](https://ffmpeg.org/doxygen/8.1/videotoolboxenc_8c_source.html) поддерживает постоянное качество VideoToolbox на Apple Silicon.
