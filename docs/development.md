# Разработка, CI и выпуск LauraBoard

## Автоматические проверки

`.github/workflows/ci.yml` запускается при push в ветку, pull request и вручную через **Actions → CI → Run workflow**. Этот же workflow вызывается перед релизом.

- `cargo fmt --all -- --check` проверяет форматирование по `rustfmt.toml`.
- Clippy проверяет весь workspace, все targets Cargo и все features. Предупреждения видны в журнале; ошибки останавливают проверку.
- Модульные и интеграционные тесты выполняются нативно на Windows и Linux под x86-64 и ARM64. В релизном macOS job под Intel и Apple Silicon запускаются тесты workspace с GUI и собираются нативные пакеты.
- Отдельная проверка собирает CLI без GUI на минимальной заявленной версии Rust 1.92.
- FFmpeg-тесты на Ubuntu 24.04 и 26.04 выполняют обычно пропускаемые регрессии синхронизации аудио/видео и начала/конца эфира. Таким образом проверяются поставляемые этими ОС версии FFmpeg 6.1 и 8.0. Нативные WebView-тесты запускаются на той же машине, что и остальные тесты соответствующей ОС и архитектуры; на Linux используется Xvfb. Для локального запуска сначала выполните `cargo build -p lb-webview`, затем `python .github/scripts/fetch_web_runtime.py --output target/debug/runtime`, затем `cargo test -p lb-gfx web::tests -- --ignored --test-threads=1`. Для проверки своего SWF задайте `LB_TEST_SWF` с путём к файлу.
- Actionlint проверяет workflows; Python-тесты проверяют архивы, версии и полноту релизных файлов.
- На небольшом ELF-файле дополнительно проверяются настоящие DEB, RPM и AppImage, их распаковка и запуск, а также выполнение `package()` из AUR PKGBUILD.

Основная версия Rust в Actions закреплена на **1.96.0**, Python для сборки релизов — **3.12**. При смене Rust обновляйте значения в `.github/actions/setup/action.yml` и `.github/scripts/build_matrix.py` и повторяйте проверки. Изменения зависимостей должны сопровождаться актуальным `Cargo.lock`; сборки используют `--locked`.

Общий setup подбирает параллелизм Cargo по CPU и RAM runner и отключает отладочные символы профилей dev/test на всех ОС. Тесты, debug assertions и проверки переполнения остаются включены. Параметры и объём RAM выводятся в журнал; подробности настройки Free runners приведены ниже.

На x64 Linux-runners зеркало `azure.archive.ubuntu.com` заменяется основным архивом `https://archive.ubuntu.com/ubuntu`; обрабатываются `sources.list`, файлы `.list`/`.sources` и локальные mirrorlists, указанные через `mirror+file:`, включая `/etc/apt/apt-mirrors.txt` в образе Ubuntu 24.04. Источники ARM и сторонние репозитории сохраняются. Для APT заданы сетевые таймауты 30 секунд, две повторные попытки и ожидание блокировки до 60 секунд. Установка зависимостей ограничена 10 минутами, FFmpeg — 5 минутами. Установка неинтерактивная; needrestart только перечисляет требующие перезапуска службы. Это предотвращает расход всего 30-минутного лимита media-job на медленное скачивание пакетов.

`CI passed` — итоговая проверка результатов остальных групп: она не запускает тесты самостоятельно. Если она сообщает о `test: failure`, откройте конкретные jobs группы Tests. Код 143 вместе с сообщением `The runner has received a shutdown signal` означает остановку runner; сам по себе этот лог не доказывает OOM или ошибку программы. Для такого job нужен повторный запуск. Обычные падения тестов следует исправлять по сообщениям соответствующего теста.

Для защиты основной ветки включите ruleset/branch protection в настройках GitHub и сделайте обязательным статус **CI passed**. Файлы workflow сами по себе не включают защиту ветки. Статус **RustSec audit** также можно сделать обязательным: он выполняется для каждого PR.

## Матрица релизов

| Система | Процессор | Runner GitHub | Файлы |
|---|---|---|---|
| Windows | x86-64 | `windows-2022` | установщик `.exe`, portable desktop `.zip`, server `.zip` |
| Windows | x86 (32-bit) | `windows-2022`, Rust host i686 | установщик `.exe`, portable desktop `.zip`, server `.zip` |
| Windows | ARM64 | `windows-11-arm` | установщик `.exe`, portable desktop `.zip`, server `.zip` |
| Linux | x86-64 | `ubuntu-24.04` | setup, portable desktop, server, AppImage, DEB, RPM, AUR |
| Linux | x86 (32-bit) | `ubuntu-24.04`, Rust host i686 + i386 libraries | setup, portable desktop, server, AppImage, DEB, RPM, AUR |
| Linux | ARM64 | `ubuntu-24.04-arm` | setup, portable desktop, server, AppImage, DEB, RPM, AUR |
| macOS | Intel x86-64 | `macos-15-intel` | `.app`, DMG, desktop/server `.tar.gz` |
| macOS | Apple Silicon ARM64 | `macos-15` | `.app`, DMG, desktop/server `.tar.gz` |

Архитектура Rust host проверяется перед упаковкой. Для x86 используется 32-битный toolchain i686 на x64 runner; Linux дополнительно получает multilib и i386-библиотеки. ARM означает ARM64 (`aarch64`), не ARMv7. macOS получает нативный GUI в `.app`/DMG, серверный архив и пользовательский launchd-агент; подробности в [macOS](macos.md). Linux-пакеты собраны на Ubuntu 24.04 и требуют совместимых системных библиотек; это не статические musl-сборки. AppImage включает зависимости через linuxdeploy, но требует совместимой glibc. На других дистрибутивах совместимость нужно проверить отдельно.

Для Windows/Linux workflow **Release** сохраняет прежний состав FFmpeg; macOS-пакеты содержат FFmpeg/ffprobe и перемещаемые библиотеки. Для комплектов с FFmpeg используйте workflows ниже. Подпись Windows-кода и notarization macOS не настроены. Перед первым публичным релизом проверьте установку, обновление и удаление на целевых ОС: CI проверяет сборку, тесты и запуск CLI, но не проходит интерактивный мастер.

## Отдельные сборки Linux и Windows

В **Actions → Run workflow** доступны **Build Linux**, **Build Windows** и **Build with FFmpeg**. Выбор `architecture`: `all`, `x64`, `arm64`; для Linux также доступен `x86`. Выбор `package_format`: `all`, `setup`, `portable`, `server`, а на Linux также `appimage`, `debpackage`, `rpmpackage`, `aur`. Третий workflow позволяет выбрать `platform`: `all`, `linux`, `windows`. По умолчанию собираются все архитектуры и форматы. Несовместимый выбор (например Windows + AppImage) отклоняется до компиляции.

Для каждой ОС и архитектуры один job устанавливает зависимости, запускает desktop/server- и WebView-тесты, собирает приложение и последовательно упаковывает выбранные форматы. Установка библиотек и передача промежуточных архивов между jobs не повторяются. Итоговые artifacts разделены по формату: например `windows-setup-arm64`, `windows-portable-arm64`, `linux-appimage-x64`; внутри каждого ровно один файл. Срок хранения ручных artifacts — 3 дня, промежуточных artifacts релиза — 1 день; опубликованные файлы GitHub Release сохраняются отдельно. Ручные сборки не публикуют GitHub Release.

В Release нативные тесты выполняются в сборочных jobs; CI вызывается в режиме `checks_only` для Clippy, MSRV и FFmpeg-интеграций. Эти проверки и аудит идут параллельно сборкам. Публикация ждёт успешного завершения всех групп. Обычный CI при push/PR выполняет нативные тесты самостоятельно.

Cargo-кэш включает повторно используемые зависимости из `target` и `target/server`; собственные крейты workspace не сохраняются, чтобы большие GUI-артефакты не вытесняли зависимости из бесплатного кэша. Кэш сохраняется также после ошибки упаковки. Ключи разделяют ОС, архитектуру, toolchain, параметры компилятора и вид прогона: тесты либо тесты со сборкой. Это позволяет сохранить release-кэш независимо от более раннего CI, где есть только debug-артефакты. Debug и release остаются отдельными профилями Cargo и не подменяют друг друга.

Параметры стандартных runners задаёт `.github/scripts/ci_resources.py`: GUI-компиляция использует до двух процессов Cargo, по одному на каждые 6 GiB RAM. Поэтому на runners с 14–16 GiB запускаются два процесса, на macOS ARM64 с 7 GiB и малых private runners — один. Серверные проверки используют до четырёх процессов с резервом 2 GiB на процесс, x86 — один. Debug-информация dev/test отключена для всех ОС. MSRV, аудит и FFmpeg-интеграции не устанавливают GUI-библиотеки; rustfmt/clippy устанавливаются только для quality.

Для сборочных jobs Actions включён `fast-release`: `opt-level=3` сохраняется, LTO отключён, `codegen-units=16`. Это сокращает оптимизацию при компоновке, но может увеличить размер бинарника и уменьшить выигрыш от межмодульной оптимизации. Локальный `[profile.release]` в `Cargo.toml` не изменён. Homebrew не выполняет автоматическое обновление/очистку при установке FFmpeg. Новые ручные сборки отменяют устаревшие для той же ветки; релизные сборки не отменяются.

После изменения параметров первый запуск создаёт новый совместимый кэш. Ускорение и пик RAM следует оценивать по последующим запускам Actions; фиксированный процент ускорения не гарантируется.

DEB и RPM устанавливают приложение в `/opt/lauraboard`, команду в `/usr/bin` и пункт меню с иконкой. Удалением управляет пакетный менеджер; пользовательский `uninstall` в эти пакеты не входит. AUR-артефакт — самостоятельный архив с `PKGBUILD`, `.SRCINFO`, desktop-файлом и portable-payload с SHA256: распакуйте его и выполните `makepkg -si`. Автопубликация в AUR не выполняется. x86/i686 и ARM64 предназначены для Arch Linux 32 и Arch Linux ARM; официальный Arch Linux использует x86-64.

Сборки и **Release** используют общий `.github/workflows/build-ffmpeg.yml` и матрицу `.github/scripts/build_matrix.py`. Лимит общего job тестирования, сборки и упаковки — **360 минут**; `fail-fast: false` позволяет соседним заданиям матрицы завершиться независимо.

Все три ручных workflow включают FFmpeg и ffprobe по умолчанию (`include_ffmpeg: true`); флажок можно отключить. Для x64/ARM64 FFmpeg берётся из статических GPL-сборок ветки 8.1 [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest). Перед использованием проверяется SHA256 по upstream `checksums.sha256`, затем запускаются обе утилиты с `-version`. Windows x86 не собирается. Для ручных Linux x86-сборок FFmpeg не включается: используются системные утилиты. Лицензии и provenance входят в `licenses/ffmpeg`. Тег BtbN `latest` меняется, поэтому точный комплект фиксируется в provenance каждой сборки.

LauraBoard ищет комплектные утилиты рядом со своим исполняемым файлом, включая создание демо-проекта; установка в системный PATH не требуется. Явные пути в `[ffmpeg]` сохраняют приоритет. Мастер установки распознаёт FFmpeg внутри своего пакета и позволяет создать демо-проект без предварительной установки системного FFmpeg.

## Выпуск версии

Публикация использует секрет `RELEASE_TOKEN`, если он настроен; иначе — встроенный `GITHUB_TOKEN` с `contents: write` только в job `publish`. Если после успешной генерации notes команда `gh release create` возвращает `HTTP 403: Resource not accessible by integration`, проверьте права токена и правила создания тегов. Для целевого коммита, меняющего `.github/workflows/` относительно default branch, GitHub может потребовать право **Workflows: write**, которое нельзя выдать `GITHUB_TOKEN` через YAML ([документация GitHub](https://docs.github.com/en/rest/releases/releases#create-a-release)). Создайте fine-grained PAT с доступом только к этому репозиторию и правами **Contents: Read and write** и **Workflows: Read and write**; сохраните его в **Settings → Secrets and variables → Actions → New repository secret** под именем `RELEASE_TOKEN`. Токен используется только на шаге публикации. После изменения workflow запустите новый **Run workflow**; повтор старого запуска не подхватит новый YAML.

1. Измените версию только в `[workspace.package].version` корневого `Cargo.toml`: все крейты наследуют её через `version.workspace = true`, а внутренние зависимости берутся из `[workspace.dependencies]` без дублирования номера версии. В том же корневом файле задаются общие `edition`, `rust-version`, `license` и `authors`. Обновите `Cargo.lock` командой `cargo update --workspace --offline` и внесите изменения в `docs/CHANGELOG.md`.
2. Отправьте изменения через PR и дождитесь CI. Для сборки без публикации используйте **Build Linux**, **Build Windows** или **Build with FFmpeg**.
3. После слияния запустите **Actions → Release → Run workflow** на нужной ветке. После успешных проверок и сборок workflow опубликует GitHub Release с тегом `v<версия из Cargo.toml>`; если тега ещё нет, он будет создан на коммите этого запуска. Повторный запуск уже завершённого workflow использует прежнюю версию workflow: после исправлений запускайте новый **Run workflow**. Альтернативный способ запуска — создать и отправить тег, точно совпадающий с версией приложения:

   ```bash
   git tag -a v0.1.0 -m "LauraBoard 0.1.0"
   git push origin v0.1.0
   ```

4. Workflow проверит версии всех крейтов, выполнит CI и RustSec audit, соберёт восемь сочетаний ОС/архитектуры и проверит комплект из **32 файлов**: Windows 3 × 3, Linux 3 × 7 и два server-архива macOS. Затем создаст GitHub Release с автоматически сгенерированными release notes и `SHA256SUMS`. Тег вида `v0.2.0-rc.1` создаёт prerelease. Если любая проверка или сборка упала, публикация не выполняется.

Архивы содержат каталог с именем сборки, бинарный файл, документацию, примеры и `build-info.json` с версией, Rust target, типом сборки, commit SHA и версией компилятора. `desktop` включает GUI; `server` собран с `--no-default-features`. Названия архивов содержат полный Rust target, например `lauraboard-0.1.0-aarch64-unknown-linux-gnu-server.tar.gz`.

Описание GitHub Release начинается с таблицы кнопок скачивания по ОС: Windows и Linux — x64/x86/ARM64, macOS — Server для Intel и Apple Silicon. Кнопки ведут прямо на файлы текущего тега; отдельная ссылка открывает `SHA256SUMS`. Таблица генерируется командой `GH_REPO=owner/repository python .github/scripts/release.py notes` в `target/release-notes.md`; число и имена ссылок сверяются с ожидаемым комплектом релиза. Изображения кнопок предоставляет Shields.io. Workflow передаёт таблицу через `--notes-file`, а GitHub дописывает автоматически сгенерированный список изменений ниже.

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

Сборка установщика: `cargo dist`. Для локальной сборки setup и portable задайте `RELEASE_TARGET` равным Rust host target и выполните `python .github/scripts/release.py build`. Для дополнительной упаковки перенесите полученные файлы из `target/release-assets` в `target/build-input`, задайте `PACKAGE_FORMAT` и выполните `python .github/scripts/package.py`. Каталог `target/release-assets` перед каждым запуском должен отсутствовать или быть пустым; не задавайте `CARGO_BUILD_TARGET` и `CARGO_TARGET_DIR`. Для проверки настоящих Linux-пакетов на небольшом ELF-файле вместо GUI: `LB_PACKAGE_INTEGRATION=1 python3 .github/scripts/test_package_integration.py -v` (нужны gcc, dpkg-dev, rpm, cpio, patchelf, file, desktop-file-utils и системные библиотеки). GitHub-hosted runners начинают с чистого checkout.

Источники параметров CI: [список runners GitHub](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), [Rust cache](https://github.com/Swatinem/rust-cache), [Cargo audit](https://github.com/rustsec/rustsec/tree/main/cargo-audit).
