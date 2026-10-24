# API

Локальный HTTP API для управления эфиром, интеграции с внешними системами и панелями. По умолчанию слушает `127.0.0.1:8787` (`[api] bind` в `laurabrod.toml`). Все пути начинаются с `/api/v1`, данные передаются в JSON.

## Аутентификация и роли

- Если токены не заданы, доступ разрешён только с этого компьютера (loopback) с правами `admin`.
- С токенами: заголовок `Authorization: Bearer <токен>` (для WebSocket — параметр `?token=<токен>`).

| Роль | Права |
|---|---|
| `viewer` | чтение: состояние, рундаун, план, медиатека, отчёты, события |
| `operator` | + управление эфиром, графикой, правка медиатеки, плейлистов, сетки, правил, графики |
| `admin` | + параметры канала (`/config`, где хранятся ключи трансляций) |

Ошибки возвращаются как `{"error": "текст"}` с кодами 400, 401, 403, 404, 409 или 500.

## Общие

| Метод | Путь | Описание |
|---|---|---|
| GET | `/version` | версия |
| GET | `/system` | загрузка компьютера: процессор (общая и по ядрам), память, файл подкачки, диски (место, чтение/запись в секунду), сеть, время работы, процессы Laurabrod и FFmpeg |
| GET | `/channels` | состояние всех каналов |
| POST | `/channels` | создать канал (admin): `{"id", "name", "copy_from"}` — настройки копируются без выходов, автозапуск выключен |
| GET | `/events` | WebSocket: события всех каналов |
| GET | `/logs?lines=N` | хвост журнала программы (текст) |
| POST | `/archives/inspect` | содержимое архива канала (admin): `{"path"}` → `manifest` (канал, части, число и объём файлов) |
| POST | `/archives/import` | импорт канала из архива (admin, фоновая задача): `{"path", "id", "name"}` — `id` по умолчанию из архива; канал создаётся без автозапуска |
| GET | `/jobs`, `/jobs/{job}` | фоновые задачи (копирование медиа, экспорт и импорт каналов): `state` = `running`/`done`/`failed`/`cancelled`, `progress` 0…1, `message`, `result` |
| DELETE | `/jobs/{job}` | отменить идущую задачу или убрать завершённую из списка |

## Канал `/channels/{id}`

| Метод | Путь | Описание |
|---|---|---|
| PUT | `` (сам канал) | изменить канал (admin): `{"name", "id", "autostart"}` — пустые поля не меняются; новый `id` (имя папки) — только при остановленном эфире, пути к собственным файлам канала переписываются |
| DELETE | `?purge=false` | удалить канал (admin): папка переносится в `deleted/<id>-<время>`, с `purge=true` — стирается |
| POST | `/export` | экспорт в ZIP (admin, фоновая задача): `{"path": "D:/backup/main.zip", "parts": {"settings": true, "outputs": false, "library": true, "media": true, "playlists": true, "schedules": true, "rules": true, "graphics": true, "history": false}}` — `media` кладёт в архив сами файлы (и копии канала, и файлы из других папок), `outputs` — выходы с ключами трансляций |
| POST | `/duplicate` | копия канала со всем содержимым (admin): `{"id", "name"}` — без выходов и журналов, автозапуск выключен |
| GET | `/status` | эфир сейчас и далее, отсчёты, энкодер, графика, ошибки |
| GET | `/rundown?limit=N` | ближайший эфир с расчётным временем |
| GET | `/plan?from=…&hours=N` | прогноз на произвольный период (до 14 суток) |
| POST | `/plan` | то же по черновику: `{"from": "…", "hours": 24, "rules": {…}, "schedules": {…}}` — `rules`/`schedules` (необязательные) подменяют сохранённые только для этого расчёта, ничего не сохраняется |
| GET | `/preview.jpg` | последний кадр превью |
| GET | `/validate` | проверка конфигурации и медиа |
| GET | `/events` | WebSocket: события канала |
| POST | `/command` | команда (см. ниже) |
| POST | `/start`, `/stop`, `/skip` | короткие формы команд |
| PUT | `/var/{key}` | `{"value": "…"}` — переменная графики (`null` — удалить) |
| GET, POST | `/media` | список; добавить или заменить элемент (`MediaItem`, в том числе сцену — см. configuration.md) |
| GET, PUT, DELETE | `/media/{media}` | элемент; при удалении копия файла в хранилище канала тоже удаляется |
| POST | `/media/{media}/preview.jpg?w=480` | кадр ролика так, как он выйдет в эфир с настройками показа из тела запроса (`display`, см. library.json); не кэшируется |
| GET | `/media/{media}/thumb.jpg?w=320` | миниатюра (JPEG, кадр на 10 % длительности, ширина 32…1280). Кэшируется в `<проект>/cache/thumbs/<канал>/`, не более двух ffmpeg одновременно; 404 для потоков, аудио и отсутствующих файлов |
| POST | `/media/import` | `{"paths": [...], "kind": "ad", "category": "x", "recursive": true, "folder": "Фильмы", "copy": true}` — импорт с анализом. С `copy` файлы копируются в `channels/<id>/media/<uuid>.<ext>` фоновой задачей (ответ `{"job": id}`), исходное имя сохраняется в `original_name` |
| POST | `/media/backgrounds` | готовые фоны канала (фоновая задача, ответ `{"job": id}`): `{"stills": true, "loops": true, "plates": true}` — картинки и видеоряды в папке медиатеки «Фоны», текстуры для плашек; существующие не пересоздаются |
| GET | `/graphics/backgrounds` | текстуры для плашек: `[{"path", "title", "file"}]` (создаются при первом запросе) |
| GET | `/graphics/backgrounds/{file}` | файл текстуры (PNG) |
| GET | `/media/folders` | все папки медиатеки |
| POST | `/media/folders` | `{"op": "create"/"rename"/"delete"/"move", "path", "to", "ids"}` — папки виртуальные: файлы на диске не перемещаются; при удалении содержимое переходит в родительскую папку |
| POST | `/media/check` | перепроверка всех файлов |
| GET, POST | `/playlists` | список (с длительностями); создать |
| GET, PUT, DELETE | `/playlists/{pl}` | плейлист |
| POST | `/playlists/{pl}/entries` | `{"media": "id", "index": 0, "pinned": false}` |
| DELETE | `/playlists/{pl}/entries/{n}` | удалить позицию |
| POST | `/playlists/{pl}/move` | `{"from": 3, "to": 0}` |
| GET, POST | `/playlists/{pl}/m3u` | экспорт и импорт M3U (тело — текст M3U) |
| GET, PUT | `/schedules` | сетка (`ScheduleSet`) целиком |
| GET, PUT | `/rules` | правила (`RuleSet`) целиком |
| GET, PUT | `/graphics` | графика (`GraphicsConfig`) целиком |
| POST | `/graphics/render` | PNG с прозрачностью: `{"config": GraphicsConfig, "vars": {…}, "t_ms": 250, "width": 1280, "elements": ["id", …]}` — черновик рисуется тем же компоновщиком, что и эфир, в размере кадра канала. `t_ms` — время с момента появления (анимация; по умолчанию элементы показаны полностью), `width` — уменьшить кадр, `elements` — какие элементы (по умолчанию все верхнего уровня). Переменные — живые (`{title}`, `{next_title}` …) плюс `vars`. Ничего не сохраняется |
| GET, POST | `/fonts` | шрифты сервера, файлы шрифтов канала и скачанные семейства (`{"system", "files", "families"}`); добавить файл `{"path"}` |
| POST | `/fonts/google` | скачать семейство Google Fonts в канал: `{"family": "PT Sans"}` (обычное, жирное, курсив) |
| GET, PUT | `/config` | параметры канала (admin) |
| GET | `/asrun?from=YYYY-MM-DD&to=…&format=json\|csv` | журнал фактического эфира |
| GET | `/stats?from=…&to=…&format=json\|csv` | статистика: показы, реклама, анонсы, отклонения |

Изменения (`PUT`/`POST`) проверяются валидатором. Документ с ошибками отклоняется с кодом 400. Принятые изменения сохраняются на диск, и канал перечитывает конфигурацию.

## Команды `POST /channels/{id}/command`

```json
{"cmd": "start"}
{"cmd": "stop"}
{"cmd": "skip"}
{"cmd": "restart"}
{"cmd": "seek", "by_ms": -10000}
{"cmd": "seek", "to_ms": 750000}
{"cmd": "manual", "enabled": true}
{"cmd": "play_now", "media": "breaking-news"}
{"cmd": "queue", "media": "promo-1"}
{"cmd": "clear_queue"}
{"cmd": "ad_break", "ad_break": "regular", "now": false}
{"cmd": "graphic", "element": "logo", "action": "hide"}
{"cmd": "graphic_auto", "element": "logo"}
{"cmd": "set_var", "key": "ticker", "value": "Текст"}
{"cmd": "switch_schedule", "schedule": "holiday"}
{"cmd": "reload"}
```

`seek` перематывает текущий ролик: `by_ms` — относительно текущего места (отрицательное — назад), `to_ms` — от начала ролика. Потоки и картинки не перематываются.

Ответ: `{"ok": true, "message": "…"}`.

## События (WebSocket)

Каждое сообщение — JSON с полями `channel`, `at`, `level` (`info` | `warning` | `critical`) и `type`:

| `type` | Данные |
|---|---|
| `started`, `stopped` | — |
| `item_started` | `item` (элемент рундауна) |
| `item_ended` | `record` (запись журнала эфира: план и факт, статус) |
| `rundown_changed` | `items`, `warnings` |
| `mode_changed` | `manual` |
| `schedule_switched` | `schedule` |
| `encoder_started` / `encoder_stopped` | `codec` / `error` |
| `graphics_changed` | — |
| `message` | `text` (пропущенные файлы, сбои декодера, сторожевой таймер) |

Раз в секунду также приходит `{"type": "status", "channel": "…", "status": {...}}`.

## Примеры

```bash
curl -s http://127.0.0.1:8787/api/v1/channels/main/status
```

```bash
curl -s -X POST -H "Content-Type: application/json" -d "{\"cmd\":\"skip\"}" http://127.0.0.1:8787/api/v1/channels/main/command
```

```bash
curl -s -H "Authorization: Bearer $TOKEN" "http://tv-server:8787/api/v1/channels/main/stats?from=2026-10-01&to=2026-10-31&format=csv" -o stats.csv
```
