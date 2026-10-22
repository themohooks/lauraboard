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
| GET | `/channels` | состояние всех каналов |
| POST | `/channels` | создать канал (admin): `{"id", "name", "copy_from"}` — настройки копируются без выходов, автозапуск выключен |
| GET | `/events` | WebSocket: события всех каналов |
| GET | `/logs?lines=N` | хвост журнала программы (текст) |

## Канал `/channels/{id}`

| Метод | Путь | Описание |
|---|---|---|
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
| GET, POST | `/media` | список; добавить или заменить элемент (`MediaItem`) |
| GET, PUT, DELETE | `/media/{media}` | элемент |
| GET | `/media/{media}/thumb.jpg?w=320` | миниатюра (JPEG, кадр на 10 % длительности, ширина 32…1280). Кэшируется в `<проект>/cache/thumbs/<канал>/`, не более двух ffmpeg одновременно; 404 для потоков, аудио и отсутствующих файлов |
| POST | `/media/import` | `{"paths": [...], "kind": "ad", "category": "x", "recursive": true}` — импорт с анализом |
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
