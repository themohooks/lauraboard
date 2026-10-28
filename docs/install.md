# Установка и запуск

## Требования к компьютеру

Проверенные численные минимумы CPU, RAM, диска, версии ОС и количество каналов на конкретном оборудовании не установлены. Прежние оценки исключены: они не подтверждены измерениями проекта. Нагрузка зависит от формата канала, материалов, графики, записи и числа выходов. Проверяйте её через «Статистика → Система». См. [совместимость](compatibility.md) и [первую установку для оператора](installation.md).

## Программы

- **FFmpeg ≥ 6.1** (`ffmpeg` и `ffprobe`). Нужны опция `-readrate_initial_burst` и энкодер `s302m`, они есть в обычных сборках.
- Для сборки из исходников: **Rust ≥ 1.92** (rustup).
- Для GUI на Linux: X11 или Wayland, OpenGL (или программная отрисовка, см. [gui.md](gui.md#отрисовка)) и xdg-desktop-portal для диалогов выбора файлов. Для сервера без экрана GUI не нужен: используйте `run` и API или соберите программу без GUI.

## Установщик

Проще всего поставить LauraBoard установщиком `lauraboard-<версия>-<os>-<arch>-setup` — для Windows (`.exe`) и для Linux. Это мастер в том же стиле Material 3, что и интерфейс программы:

1. **Приветствие** — что ставится, обновление поверх старой версии, проверка FFmpeg (если его нет — подсказка, как установить).
2. **Папка** — по умолчанию `%LOCALAPPDATA%\Programs\LauraBoard` или `~/.local/share/lauraboard`. Установка только для текущего пользователя, права администратора не нужны.
3. **Параметры** — ярлык в меню «Пуск» / меню приложений, ярлык на рабочем столе, команда `lauraboard` в терминале (PATH пользователя на Windows, ссылка в `~/.local/bin` на Linux), папка проекта канала и демо-канал (нужен FFmpeg).
4. **Установка** и **Готово** — с запуском программы.

Удаление: «Параметры → Приложения» на Windows или `uninstall` в папке программы. Удаляется ровно то, что создал установщик (список хранится в `installation.json` рядом с `gui.json`); проекты, медиа и журналы остаются.

Без окна (для скриптов и развёртывания):

```bash
lauraboard-setup --silent --dir D:\Apps\LauraBoard --project D:\tv --no-desktop
```

```bash
uninstall --uninstall --silent
```

Остальные ключи: `--no-menu`, `--no-path`, `--no-project`, `--demo`, `--forget-settings` (с `--uninstall`: удалить и настройки интерфейса), `--remove-data` (с `--uninstall`: удалить данные каналов и эфиров проекта — настройки, плейлисты, сетку, журналы; файлы внутри удаляемого channels/ тоже удаляются; заранее проверьте расположение собственных медиа и записей); `--help` — полный список.

### Сборка установщика

Одна команда собирает release-версию программы и установщик с ней внутри:

```bash
cargo dist
```

Результат — `target/dist/lauraboard-<версия>-<os>-<arch>-setup[.exe]`. Установщик собирается для той ОС, на которой запущена команда: Windows-версию собирайте на Windows, Linux-версию — на Linux (нужны те же зависимости, что и для сборки GUI, см. ниже).

Как это устроено (крейты `xtask`, `lb-installer`, `lb-winres`):

- `lauraboard-setup` без вложенного архива работает только как деинсталлятор — эта копия кладётся в пакет как `uninstall[.exe]`;
- программа, деинсталлятор, иконка и лицензии упаковываются (README и документация остаются на GitHub) в zip с `manifest.json` (версия, ОС, размер) и встраиваются в `lauraboard-setup` при сборке (`LB_PAYLOAD`);
- на Windows оба exe получают иконку и манифест (`asInvoker`, DPI) без Windows SDK; иконка `.ico` генерируется из `crates/lb-gui/assets/app/lauraboard.svg` командой `cargo xtask icons`;
- страницы мастера можно отрисовать в PNG без экрана: `cargo run -p lb-installer --features snapshot -- --snapshot target/snapshots/setup`.

## Сборка из исходников

### Windows

1. Установите FFmpeg:
   ```bash
   winget install Gyan.FFmpeg
   ```
   Можно скачать сборку с gyan.dev и прописать путь в `lauraboard.toml`:
   ```toml
   [ffmpeg]
   ffmpeg = "C:/ffmpeg/bin/ffmpeg.exe"
   ffprobe = "C:/ffmpeg/bin/ffprobe.exe"
   ```
2. Соберите программу:
   ```bash
   cargo build --release
   ```
   Исполняемый файл появится в `target\release\lauraboard.exe`.
3. Создайте проект и запустите:
   ```bash
   lauraboard -p D:\tv init --demo
   ```
   ```bash
   lauraboard -p D:\tv gui
   ```

#### Служба Windows

Из командной строки, запущенной от имени администратора:

```bash
lauraboard -p D:\tv service install --name lauraboard
```

Служба запускается автоматически при старте системы и перезапускается при сбое. Удаление:

```bash
lauraboard service uninstall --name lauraboard
```

Пока служба работает, подключайтесь к ней из GUI командой `lauraboard -p D:\tv gui`: GUI найдёт работающий API и не станет запускать второй движок.

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
   ./target/release/lauraboard -p /srv/tv init --demo
   ```
   ```bash
   ./target/release/lauraboard -p /srv/tv run
   ```

#### Сервис systemd

```bash
sudo ./target/release/lauraboard -p /srv/tv service install
```

Команда создаёт `/etc/systemd/system/lauraboard.service`, включает и запускает его. Без прав root unit-файл выводится на экран, установите его вручную:

```ini
[Unit]
Description=LauraBoard TV playout
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/lauraboard --project /srv/tv run
WorkingDirectory=/srv/tv
Restart=always
RestartSec=3
KillSignal=SIGINT
TimeoutStopSec=20

[Install]
WantedBy=multi-user.target
```

Логи: `journalctl -u lauraboard -f` и файлы в `<проект>/logs/`.

## Перенос между Windows и Linux

Пути к медиа хранятся относительно корня проекта и с прямыми слешами. Каталог проекта (или архив `lauraboard backup`) можно перенести на другую ОС без изменений. Абсолютные пути (`C:/media/...`) после переноса придётся поправить.

## Первые шаги после установки

1. Добавьте свои ролики:
   ```bash
   lauraboard media add D:/video --recursive --category shows
   ```
2. Создайте плейлист:
   ```bash
   lauraboard playlist create day
   ```
   Затем добавьте в него ролики командой `lauraboard playlist add day <id...>`.
3. Опишите сетку в `channels/<id>/schedule.toml` (см. [configuration.md](configuration.md)) или используйте режим `sequential`.
4. Пропишите выход в `channel.toml`, например RTMP-сервер или YouTube.
5. Проверьте настройки:
   ```bash
   lauraboard validate
   ```
   Затем посмотрите прогноз эфира:
   ```bash
   lauraboard plan --hours 12
   ```
6. Запустите `lauraboard run` или `lauraboard gui`.
