# Установка и запуск

## Требования

- **FFmpeg ≥ 6.1** (`ffmpeg` и `ffprobe`). Нужны опция `-readrate_initial_burst` и энкодер `s302m`, они есть в обычных сборках.
- Для сборки из исходников: **Rust ≥ 1.92** (rustup).
- Для GUI на Linux: X11 или Wayland и драйвер OpenGL/Vulkan. Для сервера без экрана GUI не нужен: используйте `run` и API.

## Windows

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

### Служба Windows

Из командной строки, запущенной от имени администратора:

```bash
laurabrod -p D:\tv service install --name laurabrod
```

Служба запускается автоматически при старте системы и перезапускается при сбое. Удаление:

```bash
laurabrod service uninstall --name laurabrod
```

Пока служба работает, подключайтесь к ней из GUI командой `laurabrod -p D:\tv gui`: GUI найдёт работающий API и не станет запускать второй движок.

## Linux

1. Установите FFmpeg. В Debian и Ubuntu:
   ```bash
   sudo apt install ffmpeg
   ```
2. Установите зависимости сборки GUI (Debian/Ubuntu):
   ```bash
   sudo apt install build-essential pkg-config libxkbcommon-dev libgl1-mesa-dev libwayland-dev
   ```
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

### Сервис systemd

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
