# Командная строка

```
laurabrod [-p ПРОЕКТ] [-c КАНАЛ] [--token ТОКЕН] [--json] <команда>
```

Общие параметры (можно задать переменными окружения):

| Параметр | Переменная | Значение |
|---|---|---|
| `-p, --project` | `LAURABROD_PROJECT` | каталог проекта (по умолчанию `.`) |
| `-c, --channel` | `LAURABROD_CHANNEL` | канал (по умолчанию первый) |
| `--token` | `LAURABROD_TOKEN` | токен API для `ctl` (по умолчанию — токен с наибольшей ролью из конфигурации) |
| `--json` | | машиночитаемый вывод (где поддерживается) |

## Команды

| Команда | Описание |
|---|---|
| `init [--id main] [--demo]` | создать проект (`--demo` — с демо-каналом и тестовыми роликами) |
| `run [--no-api]` | запустить движок без GUI (передний план; для служб и скриптов) |
| `gui [--connect URL]` | интерфейс; без `--connect` подключается к работающему экземпляру или запускает встроенный движок |
| `validate [--no-media]` | проверить конфигурацию и медиафайлы |
| `plan [--from ДАТА] [--hours N] [--format table\|json\|csv] [--programs-only]` | прогноз эфира |
| `media list [--kind K]` | медиатека |
| `media add ПУТИ... [--kind K] [--category C] [-r]` | добавить файлы, папки или URL (с анализом ffprobe) |
| `media remove ID` | удалить из медиатеки |
| `media set ID [--title] [--kind] [--category] [--tags a,b] [--volume-db] [--promo-block B]` | изменить свойства |
| `media check` | перепроверить все файлы (удалённые и повреждённые) |
| `playlist list \| show ID` | плейлисты |
| `playlist create ID [--name] [--no-loop] [--shuffle]` | создать |
| `playlist add ID MEDIA... [--pinned]` | добавить ролики |
| `playlist remove-entry ID N` / `move ID FROM TO` | редактирование (позиции с 0) |
| `playlist delete ID [--yes]` / `clear ID [--yes]` | удаление и очистка (с подтверждением) |
| `playlist import ID FILE.m3u` / `export ID [FILE]` | импорт и экспорт M3U |
| `schedule list` | сетки и шаблоны |
| `schedule assign ШАБЛОН ДНИ [--schedule S]` | назначить шаблон дням (`mon,tue`, `weekdays`, `weekend`, `all`) |
| `schedule duplicate ИЗ В` | копия шаблона дня |
| `schedule exception ДАТА ШАБЛОН\|none` | исключение на дату |
| `schedule export [FILE]` / `import FILE` | экспорт и импорт сетки (JSON) |
| `report [asrun\|stats] [--from] [--to] [--format table\|json\|csv] [--out FILE]` | журнал эфира и статистика |
| `backup [--out FILE.zip]` / `restore FILE.zip [--force]` | резервная копия конфигурации |
| `ctl …` | управление работающим экземпляром (ниже) |
| `service install\|uninstall [--name N]` | служба Windows или systemd |

Команды, которые меняют файлы (`media`, `playlist`, `schedule`), после сохранения просят работающий экземпляр перечитать конфигурацию.

## ctl — управление эфиром через API

| Команда | Действие |
|---|---|
| `ctl status` / `ctl watch` | состояние эфира (`watch` — обновление каждую секунду) |
| `ctl start` / `ctl stop [--yes]` | старт и остановка вещания |
| `ctl skip` / `ctl restart` | пропустить или повторить ролик |
| `ctl seek +30` / `ctl seek -10` / `ctl seek 00:12:30` | перемотать текущий ролик на N секунд вперёд / назад или на позицию от начала |
| `ctl manual on\|off` | приостановить расписание (ручной режим) или вернуться к нему |
| `ctl play MEDIA` / `ctl queue MEDIA` / `ctl clear-queue` | показать сейчас, поставить в очередь, очистить очередь |
| `ctl adbreak ID [--now]` | внеплановая реклама |
| `ctl graphic ЭЛЕМЕНТ show\|hide\|auto` | ручное управление графикой |
| `ctl var КЛЮЧ [ЗНАЧЕНИЕ]` | переменная графики (например `ticker`); без значения — удалить |
| `ctl schedule ID\|auto` | переключить сетку |
| `ctl reload` | перечитать конфигурацию |
| `ctl rundown [--limit N]` / `ctl logs [--lines N]` | ближайший эфир и журнал программы |

## Коды завершения

| Код | Значение |
|---|---|
| 0 | успех |
| 1 | общая ошибка |
| 2 | ошибки конфигурации или проверки |
| 3 | не найдено (канал, ролик, плейлист) |
| 4 | API недоступен или нет прав |
| 5 | канал уже запущен другим процессом |
| 6 | отменено пользователем |

## Примеры для скриптов

```bash
laurabrod -p /srv/tv validate --no-media && laurabrod -p /srv/tv ctl reload
```

```bash
laurabrod -p /srv/tv ctl var ticker "Срочно: штормовое предупреждение"
```

```bash
laurabrod -p /srv/tv report asrun --from 2026-10-01 --to 2026-10-31 --format csv --out october.csv
```

```bash
laurabrod -p /srv/tv --json ctl status
```
