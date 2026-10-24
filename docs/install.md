# Установка и запуск

## Требования к компьютеру

Нагрузку создаёт кодирование видео: на каждый канал работает один кодировщик FFmpeg (x264 на процессоре или аппаратный), плюс декодер текущего ролика и графика. Цифры ниже — для одного канала с выходом на 1–2 адреса, графикой и превью.

| | Минимальные | Рекомендуемые |
|---|---|---|
| Операционная система | Windows 10 22H2 (64 бит) или Windows Server 2019; Linux x86-64 с glibc ≥ 2.31 (Ubuntu 20.04, Debian 11) | Windows 11 или Windows Server 2022; Ubuntu 22.04 / 24.04 LTS, Debian 12 |
| Процессор | 4 ядра x86-64 с AVX2, от 2,5 ГГц (Intel Core i3-8100, AMD Ryzen 3 3200G) — канал 720p25, x264 `veryfast` | 8 ядер / 16 потоков от 3,5 ГГц (AMD Ryzen 7 5700X, Intel Core i7-12700) — канал 1080p50 или 2–3 канала 1080p25; на каждый следующий канал 1080p25 ≈ +4 ядра |
| Оперативная память | 4 ГБ (8 ГБ с интерфейсом на том же компьютере) | 16 ГБ; 32 ГБ для 4 и более каналов |
| Видеокарта | не нужна для эфира; для интерфейса — любая с OpenGL 3.3 или DirectX 11 (встроенная Intel HD 4000, AMD Vega), либо программная отрисовка | встроенная Intel UHD 630 / Iris Xe или дискретная; для аппаратного кодирования — NVIDIA GeForce GTX 1650 и новее (NVENC, кодек `h264_nvenc`), Intel с Quick Sync (`h264_qsv`), AMD (`h264_amf`), на Linux — VA-API (`h264_vaapi`) |
| Диск | 2 ГБ под программу и FFmpeg + место для медиа; HDD 7200 об/мин | SSD для медиатеки и записи эфира; запись 1080p при 6 Мбит/с — около 2,7 ГБ в час |
| Сеть | исходящий канал не меньше битрейта всех выходов × 1,5 (2500 кбит/с → от 4 Мбит/с) | проводное подключение, запас ×2 по битрейту; доступ в интернет для Google Fonts |

С аппаратным кодированием (NVENC, Quick Sync, AMF) процессорная нагрузка падает в 3–5 раз: канал 1080p50 идёт и на 4-ядерном процессоре. Статистика → «Система» показывает загрузку процессора, памяти и дисков по процессам Laurabrod и FFmpeg — по ней видно, есть ли запас для ещё одного канала.

## Программы

- **FFmpeg ≥ 6.1** (`ffmpeg` и `ffprobe`). Нужны опция `-readrate_initial_burst` и энкодер `s302m`, они есть в обычных сборках.
- Для сборки из исходников: **Rust ≥ 1.92** (rustup).
- Для GUI на Linux: X11 или Wayland, OpenGL (или программная отрисовка, см. [gui.md](gui.md#отрисовка)) и xdg-desktop-portal для диалогов выбора файлов. Для сервера без экрана GUI не нужен: используйте `run` и API или соберите программу без GUI.

## Установщик

Проще всего поставить Laurabrod установщиком `laurabrod-<версия>-<os>-<arch>-setup` — для Windows (`.exe`) и для Linux. Это мастер в том же стиле Material 3, что и интерфейс программы:

1. **Приветствие** — что ставится, обновление поверх старой версии, проверка FFmpeg (если его нет — подсказка, как установить).
2. **Папка** — по умолчанию `%LOCALAPPDATA%\Programs\Laurabrod` или `~/.local/share/laurabrod`. Установка только для текущего пользователя, права администратора не нужны.
3. **Параметры** — ярлык в меню «Пуск» / меню приложений, ярлык на рабочем столе, команда `laurabrod` в терминале (PATH пользователя на Windows, ссылка в `~/.local/bin` на Linux), папка проекта канала и демо-канал (нужен FFmpeg).
4. **Установка** и **Готово** — с запуском программы.

Удаление: «Параметры → Приложения» на Windows или `uninstall` в папке программы. Удаляется ровно то, что создал установщик (список хранится в `installation.json` рядом с `gui.json`); проекты, медиа и журналы остаются.

Без окна (для скриптов и развёртывания):

```bash
laurabrod-setup --silent --dir D:\Apps\Laurabrod --project D:\tv --no-desktop
```

```bash
uninstall --uninstall --silent
```

Остальные ключи: `--no-menu`, `--no-path`, `--no-project`, `--demo`, `--forget-settings` (с `--uninstall`: удалить и настройки интерфейса), `--remove-data` (с `--uninstall`: удалить данные каналов и эфиров проекта — настройки, плейлисты, сетку, журналы; медиафайлы и записи остаются); `--help` — полный список.

### Сборка установщика

Одна команда собирает release-версию программы и установщик с ней внутри:

```bash
cargo dist
```

Результат — `target/dist/laurabrod-<версия>-<os>-<arch>-setup[.exe]`. Установщик собирается для той ОС, на которой запущена команда: Windows-версию собирайте на Windows, Linux-версию — на Linux (нужны те же зависимости, что и для сборки GUI, см. ниже).

Как это устроено (крейты `xtask`, `lb-installer`, `lb-winres`):

- `laurabrod-setup` без вложенного архива работает только как деинсталлятор — эта копия кладётся в пакет как `uninstall[.exe]`;
- программа, деинсталлятор, иконка, README, документация и лицензии упаковываются в zip с `manifest.json` (версия, ОС, размер) и встраиваются в `laurabrod-setup` при сборке (`LB_PAYLOAD`);
- на Windows оба exe получают иконку и манифест (`asInvoker`, DPI) без Windows SDK; иконка `.ico` генерируется из `crates/lb-gui/assets/app/laurabrod.svg` командой `cargo xtask icons`;
- страницы мастера можно отрисовать в PNG без экрана: `cargo run -p lb-installer --features snapshot -- --snapshot target/snapshots/setup`.

## Сборка из исходников

### Windows

1. Установите FFmpeg:
   ```bash
   winget install Gyan.FFmpeg
   ```
   Можно скачать сборку с gyan.dev и прописать путь в `laurabrod.toml`:
   ```toml
   [ffmpeg]
   ffmpeg = "C:/ffmpeg/bin/ffmpeg.exe"
   ffprobe = "C:/ffmpeg/bin/ffprobe.exe"
   ```
2. Соберите программу:
   ```bash
   cargo build --release
   ```
   Исполняемый файл появится в `target\release\laurabrod.exe`.
3. Создайте проект и запустите:
   ```bash
   laurabrod -p D:\tv init --demo
   ```
   ```bash
   laurabrod -p D:\tv gui
   ```

#### Служба Windows

Из командной строки, запущенной от имени администратора:

```bash
laurabrod -p D:\tv service install --name laurabrod
```

Служба запускается автоматически при старте системы и перезапускается при сбое. Удаление:

```bash
laurabrod service uninstall --name laurabrod
```

Пока служба работает, подключайтесь к ней из GUI командой `laurabrod -p D:\tv gui`: GUI найдёт работающий API и не станет запускать второй движок.

### Linux

1. Установите FFmpeg. В Debian и Ubuntu:
   ```bash
   sudo apt install ffmpeg
   ```
2. Установите зависимости сборки GUI (Debian/Ubuntu):
   ```bash
   sudo apt install build-essential pkg-config libfontconfig-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libgl1-mesa-dev
   ```
   В Fedora: `sudo dnf install gcc pkgconf fontconfig-devel libxkbcommon-devel libxkbcommon-x11-devel wayland-devel mesa-libGL-devel`.
3. Соберите программу:
   ```bash
   cargo build --release
   ```
   Для сборки без GUI (сервер):
   ```bash
   cargo build --release --no-default-features
   ```
4. Создайте проект и запустите:
   ```bash
   ./target/release/laurabrod -p /srv/tv init --demo
   ```
   ```bash
   ./target/release/laurabrod -p /srv/tv run
   ```

#### Сервис systemd

```bash
sudo ./target/release/laurabrod -p /srv/tv service install
```

Команда создаёт `/etc/systemd/system/laurabrod.service`, включает и запускает его. Без прав root unit-файл выводится на экран, установите его вручную:

```ini
[Unit]
Description=Laurabrod TV playout
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/laurabrod --project /srv/tv run
WorkingDirectory=/srv/tv
Restart=always
RestartSec=3
KillSignal=SIGINT
TimeoutStopSec=20

[Install]
WantedBy=multi-user.target
```

Логи: `journalctl -u laurabrod -f` и файлы в `<проект>/logs/`.

## Перенос между Windows и Linux

Пути к медиа хранятся относительно корня проекта и с прямыми слешами. Каталог проекта (или архив `laurabrod backup`) можно перенести на другую ОС без изменений. Абсолютные пути (`C:/media/...`) после переноса придётся поправить.

## Первые шаги после установки

1. Добавьте свои ролики:
   ```bash
   laurabrod media add D:/video --recursive --category shows
   ```
2. Создайте плейлист:
   ```bash
   laurabrod playlist create day
   ```
   Затем добавьте в него ролики командой `laurabrod playlist add day <id...>`.
3. Опишите сетку в `channels/<id>/schedule.toml` (см. [configuration.md](configuration.md)) или используйте режим `sequential`.
4. Пропишите выход в `channel.toml`, например RTMP-сервер или YouTube.
5. Проверьте настройки:
   ```bash
   laurabrod validate
   ```
   Затем посмотрите прогноз эфира:
   ```bash
   laurabrod plan --hours 12
   ```
6. Запустите `laurabrod run` или `laurabrod gui`.
