# Разработка, CI и выпуск LauraBoard

## Автоматические проверки

`.github/workflows/ci.yml` запускается при push в ветку, pull request и вручную через **Actions → CI → Run workflow**. Этот же workflow вызывается перед релизом.

- `cargo fmt --all -- --check` проверяет форматирование по `rustfmt.toml`.
- Clippy проверяет весь workspace, все targets Cargo и все features. Предупреждения видны в журнале; ошибки останавливают проверку.
- Модульные и интеграционные тесты выполняются нативно на Windows и Linux под x86-64 и ARM64. На macOS под Intel и Apple Silicon проверяется серверная сборка без GUI.
- Отдельная проверка собирает CLI без GUI на минимальной заявленной версии Rust 1.92.
- FFmpeg-тесты на Ubuntu 24.04 и 26.04 выполняют обычно пропускаемые регрессии синхронизации аудио/видео и начала/конца эфира. Таким образом проверяются поставляемые этими ОС версии FFmpeg 6.1 и 8.0. Браузерные тесты с Chromium/Ruffle автоматически не запускаются.
- Actionlint проверяет workflows; Python-тесты проверяют архивы, версии и полноту релизных файлов.

Основная версия Rust в Actions закреплена на **1.96.0**, Python для сборки релизов — **3.12**. При смене Rust обновляйте значение в `.github/actions/setup/action.yml` и повторяйте проверки. Изменения зависимостей должны сопровождаться актуальным `Cargo.lock`; сборки используют `--locked`.

На Linux общий setup ограничивает Cargo одним параллельным job и отключает отладочные символы профилей dev/test, чтобы уменьшить расход памяти и диска при компиляции большого сгенерированного GUI. Тесты, debug assertions и проверки переполнения при этом остаются включены. Release-профиль не меняется. Начальный объём доступной памяти и места на диске выводится в журнал.

На x64 Linux-runners зеркало `azure.archive.ubuntu.com` заменяется основным архивом `https://archive.ubuntu.com/ubuntu`; обрабатываются `sources.list`, файлы `.list`/`.sources` и локальные mirrorlists, указанные через `mirror+file:`, включая `/etc/apt/apt-mirrors.txt` в образе Ubuntu 24.04. Источники ARM и сторонние репозитории сохраняются. Для APT заданы сетевые таймауты 30 секунд, две повторные попытки и ожидание блокировки до 60 секунд. Установка зависимостей ограничена 10 минутами, FFmpeg — 5 минутами. Установка неинтерактивная; needrestart только перечисляет требующие перезапуска службы. Это предотвращает расход всего 30-минутного лимита media-job на медленное скачивание пакетов.

`CI passed` — итоговая проверка результатов остальных групп: она не запускает тесты самостоятельно. Если она сообщает о `test: failure`, откройте конкретные jobs группы Tests. Код 143 вместе с сообщением `The runner has received a shutdown signal` означает остановку runner; сам по себе этот лог не доказывает OOM или ошибку программы. Для такого job нужен повторный запуск. Обычные падения тестов следует исправлять по сообщениям соответствующего теста.

Для защиты основной ветки включите ruleset/branch protection в настройках GitHub и сделайте обязательным статус **CI passed**. Файлы workflow сами по себе не включают защиту ветки. Статус **RustSec audit** также можно сделать обязательным: он выполняется для каждого PR.

## Матрица релизов

| Система | Процессор | Runner GitHub | Файлы |
|---|---|---|---|
| Windows | x86-64 | `windows-2022` | установщик `.exe`, portable desktop `.zip`, server `.zip` |
| Windows | ARM64 | `windows-11-arm` | установщик `.exe`, portable desktop `.zip`, server `.zip` |
| Linux | x86-64 | `ubuntu-24.04` | установщик, portable desktop `.tar.gz`, server `.tar.gz` |
| Linux | ARM64 | `ubuntu-24.04-arm` | установщик, portable desktop `.tar.gz`, server `.tar.gz` |
| macOS | Intel x86-64 | `macos-15-intel` | server `.tar.gz` |
| macOS | Apple Silicon ARM64 | `macos-15` | server `.tar.gz` |

Сборки нативные: архитектура Rust проверяется перед упаковкой. macOS пока получает только CLI/API без GUI и системного установщика; команда `service` использует systemd и на macOS не поддерживается. Linux-пакеты собраны на Ubuntu 24.04 и требуют совместимых системных библиотек; это не статические musl-сборки. На других дистрибутивах совместимость нужно проверить отдельно.

Workflow **Release** сохраняет прежний состав: FFmpeg/ffprobe нужно установить отдельно. Для комплектов с FFmpeg используйте workflows ниже. Подпись Windows-кода и notarization macOS не настроены. Перед первым публичным релизом проверьте установку, обновление и удаление на целевых ОС: CI проверяет сборку, тесты и запуск CLI, но не проходит интерактивный мастер.

## Отдельные сборки Linux и Windows

В **Actions → Run workflow** доступны **Build Linux**, **Build Windows** и **Build with FFmpeg**. Первые два собирают выбранную ОС под x86-64 и ARM64; третий по умолчанию собирает обе ОС и позволяет выбрать `platform`. Все три включают FFmpeg и ffprobe по умолчанию (`include_ffmpeg: true`); флажок можно отключить. Они сохраняют установщик, desktop-архив и server-архив в Artifacts на 14 дней, без публикации GitHub Release.

Сборки используют общий `.github/workflows/build-ffmpeg.yml`. Лимит каждого build-job здесь и в **Release** — **360 минут**; `fail-fast: false` позволяет другим архитектурам завершиться независимо от результата соседней сборки.

FFmpeg берётся из статических GPL-сборок ветки 8.1 [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest) для нативной ОС и архитектуры. Перед использованием проверяется SHA256 по upstream `checksums.sha256`, затем запускаются обе утилиты с `-version`. В установщик и оба portable-архива входят бинарные файлы, upstream-документация/лицензии в `licenses/ffmpeg` и `provenance.json` с URL исходного архива и его SHA256. Тег upstream `latest` меняется, поэтому точный комплект фиксируется в provenance каждой сборки.

LauraBoard ищет комплектные утилиты рядом со своим исполняемым файлом, включая создание демо-проекта; установка в системный PATH не требуется. Явные пути в `[ffmpeg]` сохраняют приоритет. Мастер установки распознаёт FFmpeg внутри своего пакета и позволяет создать демо-проект без предварительной установки системного FFmpeg.

## Выпуск версии

1. Обновите `[workspace.package].version` в корневом `Cargo.toml` и версии всех крейтов в `crates/*/Cargo.toml`, включая локальные зависимости `path + version`. Обновите `Cargo.lock` командой `cargo check --workspace` и внесите изменения в `docs/CHANGELOG.md`.
2. Отправьте изменения через PR и дождитесь CI. Для сборки без публикации используйте **Build Linux**, **Build Windows** или **Build with FFmpeg**.
3. После слияния запустите **Actions → Release → Run workflow** на нужной ветке. После успешных проверок и сборок workflow опубликует GitHub Release с тегом `v<версия из Cargo.toml>`; если тега ещё нет, он будет создан на коммите этого запуска. Повторный запуск уже завершённого workflow использует прежнюю версию workflow: после исправлений запускайте новый **Run workflow**. Альтернативный способ запуска — создать и отправить тег, точно совпадающий с версией приложения:

   ```bash
   git tag -a v0.1.0 -m "LauraBoard 0.1.0"
   git push origin v0.1.0
   ```

4. Workflow проверит версии всех крейтов, выполнит CI и RustSec audit, соберёт все шесть платформ и проверит комплект из **14 файлов**. Затем создаст GitHub Release с автоматически сгенерированными release notes и `SHA256SUMS`. Тег вида `v0.2.0-rc.1` создаёт prerelease. Если любая проверка или сборка упала, публикация не выполняется.

Архивы содержат каталог с именем сборки, бинарный файл, документацию, примеры и `build-info.json` с версией, Rust target, типом сборки, commit SHA и версией компилятора. `desktop` включает GUI; `server` собран с `--no-default-features`. Названия архивов содержат полный Rust target, например `lauraboard-0.1.0-aarch64-unknown-linux-gnu-server.tar.gz`.

После загрузки файлов релиза на Linux/macOS:

```bash
sha256sum --check SHA256SUMS   # Linux; скачайте весь комплект для полной проверки
shasum -a 256 -c SHA256SUMS   # macOS
chmod +x lauraboard-0.1.0-linux-x86_64-setup
```

В Windows отдельный файл можно проверить через `Get-FileHash <файл> -Algorithm SHA256` и сопоставить результат со строкой в `SHA256SUMS`. Эти суммы проверяют целостность файлов; они не заменяют подпись издателя.

Для публикации используется встроенный `GITHUB_TOKEN`; дополнительный PAT не нужен. Право `contents: write` получает только job публикации. Actions закреплены по commit SHA; Dependabot предлагает обновления Actions и Cargo раз в неделю. Для существующего релиза workflow не перезаписывает файлы автоматически: исправления выпускайте новой версией.

## Безопасность зависимостей

`.github/workflows/security.yml` запускает `cargo audit` для каждого PR, при push с изменением зависимостей, раз в неделю, вручную и перед каждым релизом. Найденные уязвимости блокируют выполнение. Информационные предупреждения, включая неподдерживаемые транзитивные зависимости, остаются в журнале без автоматического подавления и сами по себе не блокируют релиз.

При добавлении workflows Tokio обновлён с 1.54.0 до 1.54.1 из-за [RUSTSEC-2026-0384](https://rustsec.org/advisories/RUSTSEC-2026-0384.html). Аудит также сообщает о транзитивных `bincode`, `proc-macro-error`, `ttf-parser`, `zbus_names`, `zvariant`, `zvariant_derive`, старом `glib` и отозванном `clipboard-win`. Эти предупреждения требуют отдельного обновления соответствующих библиотек верхнего уровня; workflow их не скрывает.

## Локальные команды

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked
cargo test --workspace --locked
python -m unittest discover -s .github/scripts -p 'test_*.py' -v
python .github/scripts/release.py validate
```

При установленном FFmpeg:

```bash
cargo test --locked -p lb-media --test av_sync -- --ignored --test-threads=1
cargo test --locked -p lb-engine --test broadcast_bookends -- --ignored --test-threads=1
```

Если A/V-тесты падают в CI, job сохраняет диагностические файлы в artifact `av-sync-<runner>`: входной MPEG-TS, параметры энкодера и выходной Matroska. Локально такую запись можно включить переменной `LB_AV_SYNC_ARTIFACTS`, указав каталог для файлов. Допуски синхронизации одинаковы для всех версий FFmpeg. Ресемплер включается с первого кадра (`flags=res`): это предотвращает сброс временных меток при первой компенсации дрейфа в FFmpeg 6.1, включая случаи совпадающих номинальных частот дискретизации.

Linux-трей использует `ksni` с единственным runtime `async-io`, как и Slint. Для этой зависимости отключены default features: одновременное включение `tokio` и `async-io` запрещено самой библиотекой.

Проверки GitHub Actions локально:

```bash
go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.12
actionlint
```

Сборка установщика: `cargo dist`. Полная локальная релизная упаковка: задайте `RELEASE_TARGET` равным нативному Rust target и выполните `python .github/scripts/release.py build`. Каталог `target/release-assets` перед таким запуском должен отсутствовать или быть пустым; не задавайте `CARGO_BUILD_TARGET` и `CARGO_TARGET_DIR`. GitHub-hosted runners начинают с чистого checkout.

Источники параметров CI: [список runners GitHub](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), [Rust cache](https://github.com/Swatinem/rust-cache), [Cargo audit](https://github.com/rustsec/rustsec/tree/main/cargo-audit).
