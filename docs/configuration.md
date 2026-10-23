# Конфигурация

## Структура проекта

```
<проект>/
  laurabrod.toml                 системные параметры (FFmpeg, API, журнал, уведомления)
  channels/<id>/
    channel.toml                 формат видео/аудио, кодирование, выходы, запись, резерв
    library.json                 медиатека
    playlists/<id>.json          плейлисты
    schedule.toml                сетка вещания (шаблоны дней, неделя, исключения)
    rules.toml                   правила заставок, рекламы, анонсов, заполнения
    graphics.toml                экранная графика и правила её показа
    state/                       служебное: восстановление после сбоя, блокировка
    asrun/YYYY-MM-DD.jsonl       журнал фактического эфира
  logs/                          журнал программы (ежедневные файлы)
```

Пользовательские настройки (медиатека, плейлисты, сетка, правила, графика) хранятся отдельно от системных параметров (`laurabrod.toml`, `channel.toml`) и от служебного состояния (`state/`). Все файлы пишутся атомарно: сначала во временный файл, затем переименование.

### Длительности

Везде, где ожидается длительность, допускаются форматы: число секунд (`90`, `2.5`), `"HH:MM:SS[.mmm]"`, `"MM:SS"` и строки с единицами (`"1h30m"`, `"45s"`, `"500ms"`).

### Пути

Пути пишутся относительно корня проекта (`media/news.mp4`) или абсолютно (`D:/video/a.mp4`, `/srv/video/a.mp4`). Можно указывать URL потоков: `http(s)://`, `rtmp://`, `srt://`, `udp://`.

## laurabrod.toml

```toml
channels = []            # какие каналы загружать; пусто = все из channels/

[ffmpeg]
ffmpeg = "ffmpeg"        # путь или имя в PATH
ffprobe = "ffprobe"
loglevel = "warning"

[api]
enabled = true
bind = "127.0.0.1:8787"  # 0.0.0.0:8787 — доступ из сети (обязательно задайте токены)
# без токенов доступ есть только с этого компьютера (loopback), с правами admin
[[api.tokens]]
name = "operator"
token = "длинная-случайная-строка"
role = "operator"        # viewer | operator | admin

[log]
level = "info"           # error | warn | info | debug | trace
dir = "logs"

[notify]                 # уведомления о критических событиях
webhook = "https://example.com/hook"   # POST JSON события
command = "notify.cmd"                 # команда; событие в переменной окружения LB_EVENT
```

## channel.toml

```toml
id = "main"
name = "LAURA TV"
enabled = true
autostart = true                 # начать эфир при запуске движка

[video]
width = 1280
height = 720
fps = 25.0
scale = "fit"                    # fit (поля) | fill (обрезка) | stretch
background = "#000000"           # цвет полей и заглушки

[audio]
sample_rate = 48000
channels = 2
volume_db = 0.0                  # общая громкость
normalize = false                # выравнивание громкости EBU R128 (loudnorm)

[encoder]
video_codec = "libx264"          # auto | libx264 | h264_nvenc | h264_qsv | h264_amf | h264_vaapi
preset = "veryfast"
video_bitrate = "3000k"
# maxrate = "3000k"
# bufsize = "6000k"
gop_seconds = 2.0
audio_codec = "aac"
audio_bitrate = "160k"
extra_args = []                  # дополнительные аргументы ffmpeg для выхода
rate_control = "cbr"             # cbr — постоянный битрейт | vbr — переменный (пики до maxrate) | quality — постоянное качество
crf = 23                         # качество для rate_control = "quality": 0..51, меньше — лучше (maxrate ограничивает поток)
# profile = "high"               # baseline | main | high; пусто — по умолчанию кодировщика
# level = "4.1"                  # пусто — автоматически
tune = "zerolatency"             # только libx264/libx265; "" — без tune
# bframes = 2                    # B-кадры 0..16; не задано — по умолчанию кодировщика
scene_cut = false                # ключевые кадры на смене сцены (выкл.: строго ровный GOP для HLS и платформ)
threads = 0                      # потоки кодирования; 0 — автоматически
pix_fmt = "yuv420p"              # формат пикселей выхода

[[outputs]]
name = "youtube"
url = "rtmp://a.rtmp.youtube.com/live2/КЛЮЧ"
enabled = true
# format = "flv"                 # определяется по URL: rtmp→flv, srt/udp→mpegts, .m3u8→hls

[[outputs]]
name = "hls"
url = "output/hls/live.m3u8"

[record]                         # запись эфира в файлы (с графикой и звуком)
enabled = false
dir = "recordings"               # полный путь или папка внутри папки канала
format = "mp4"                   # mp4 (фрагментированный, читается и после сбоя) | mkv | ts
segment = "01:00:00"             # длина одного файла
# max_file_mb = 2000             # максимальный размер файла: длина сокращается по битрейту кодировщика
# keep_days = 30                 # удалять записи старше N дней
# max_total_gb = 500             # держать все записи канала в этом объёме, удаляя самые старые
# prefix = "laura"               # имя файла: <prefix>-ГГГГММДД-ЧЧММСС.<ext>; по умолчанию id канала

[preview]                        # превью для GUI/API (JPEG)
enabled = true
width = 480
fps = 15.0                       # кадров в секунду (не меньше 10)

[fallback]
image = "media/standby.png"      # картинка для заглушки (иначе цвет background)
media = "promo-channel"          # резервный ролик из медиатеки, если в эфире ничего нет

[planning]
mode = "grid"                    # grid | sequential
# playlist = "main"              # для mode = "sequential"
horizon = "24:00:00"             # насколько вперёд считать рундаун
seed = 0                         # зерно для случайного порядка (детерминированный план)
history = "336:00:00"            # сколько истории учитывать (лимиты рекламы, исключение повторов)

[graphics]
enabled = true
fps = 10.0                       # устарело: графика всегда рисуется с частотой видео
# profile = "evening"            # активный профиль оформления
```

`video_codec = "auto"` выбирает первый рабочий аппаратный энкодер (NVENC, QSV, AMF, VideoToolbox, VAAPI). Если ни один не работает, используется libx264.

## library.json

```json
{ "items": [
  { "id": "news", "path": "media/news.mp4", "title": "Новости", "kind": "program",
    "category": "news", "duration": "00:00:30",
    "in_point": "00:00:02", "out_point": "00:00:28",
    "volume_db": 0.0, "weight": 1.0,
    "meta": { "presenter": "Лаура" },
    "promo": null, "enabled": true }
]}
```

| Поле | Описание |
|---|---|
| `kind` | `program`, `jingle` (заставка), `ad`, `promo`, `filler`, `image`, `stream` |
| `in_point` / `out_point` | заданная вручную обрезка (это явное разрешение резать) |
| `weight` | вес при случайном выборе |
| `meta` | произвольные поля, доступны в шаблонах графики (`{presenter}`) |
| `promo` | для анонсов: `{"block": "evening"}`, `{"category": "movies"}` или `{"media": "id"}` |
| `display` | как показывать картинку (необязательно), см. ниже |

Настройки показа `display` (всё необязательно, по умолчанию — как есть):

```json
"display": {
  "scale": "stretch",        // fit | fill | stretch | blur; по умолчанию — как в канале
  "aspect": "4:3",           // считать исходник таким по пропорциям (неверные флаги, анаморф)
  "crop": { "left": 0, "top": 0, "right": 0, "bottom": 5 },   // обрезка краёв, % исходника
  "deinterlace": true,
  "rotate": "cw90",          // none | cw90 | r180 | ccw90
  "flip_h": false, "flip_v": false,
  "brightness": 0.0,         // -1…1
  "contrast": 1.0,           // 0…2
  "saturation": 1.0,         // 0 — чёрно-белое … 3
  "gamma": 1.0,
  "sharpen": 0.0,            // 0…2
  "denoise": 0.0             // 0…10
}
```

`stretch` растягивает ролик 4:3 на весь экран 16:9; `blur` вписывает его, а поля заполняет размытой копией картинки. Режим `blur` можно задать и всему каналу (`[video] scale = "blur"`).

Длительность и параметры потока заполняются автоматически при `laurabrod media add`.

## playlists/<id>.json

```json
{ "id": "shows", "name": "Дневные передачи", "loop": true, "shuffle": false,
  "entries": [ { "media": "show-1", "pinned": true }, { "media": "show-2" }, { "media": "show-3", "disabled": true } ] }
```

- `pinned` закрепляет элемент на его позиции при перемешивании.
- В режиме `shuffle` порядок перемешивается заново на каждом круге. Один и тот же ролик не попадает в эфир дважды подряд на стыке кругов.
- Позиция в плейлисте сохраняется между блоками и перезапусками, так что сериалы продолжаются с нужной серии.

## schedule.toml

```toml
[[schedules]]
id = "main"
name = "Основная сетка"
enabled = true
# activate_at = "2026-11-01T06:00:00"   # заранее подготовленная сетка включится в это время

[schedules.week]                        # шаблон по дням недели
sat = "weekend"
sun = "weekend"
default = "weekday"

[[schedules.exceptions]]                # исключения по датам
date = "2026-12-31"
template = "new-year"                   # отсутствие template = без сетки (заглушка)

[[schedules.templates]]
id = "weekday"

[[schedules.templates.blocks]]
id = "morning"
title = "Утренний эфир"
start = "06:00:00"                      # фиксированное начало; без start — блок идёт следом
late_policy = "strict"                  # strict | finish | truncate
allow_truncate = false                  # явное разрешение обрезать ролик ради следующего блока
category = "shows"                      # категория для правил и графики
max_items = 10                          # ограничения блока (необязательно)
max_duration = "02:00:00"
exclude_recent = "06:00:00"             # не повторять показанное за этот период
rules = true                            # применять правила вставок
fill_gap = true                         # заполнять пустоту перед следующим фиксированным блоком
intro = "jingle-morning"                # заставка начала блока (id медиа)
outro = "jingle-end"
meta = { presenter = "Анна" }

[schedules.templates.blocks.source]
type = "playlist"                       # playlist | media | category | ad_break
playlist = "morning"
restart = false                         # true — каждый раз с начала плейлиста
```

Варианты источника блока:

```toml
source = { type = "media", items = ["news"] }
source = { type = "category", category = "films", kind = "program" }
source = { type = "ad_break", ad_break = "noon" }
```

## rules.toml

```toml
[[rules]]
id = "ads"
name = "Реклама каждые 20 минут"
enabled = true
priority = 5
group = "between"            # из правил одной группы в одной точке срабатывает одно, с наибольшим приоритетом
optional = true              # можно выкинуть ради точного начала следующей передачи
probability = 1.0
select = "random"            # sequential | random
no_repeat = true
skip_on_manual = true
trigger = { type = "every_minutes", minutes = 20 }
action = { type = "ad_break", ad_break = "regular" }
[rules.conditions]
exclude_categories = ["news"]
weekdays = [1, 2, 3, 4, 5]   # 1 = пн … 7 = вс
from = "08:00:00"
to = "23:00:00"
```

Триггеры:

- `every_n_items { n }` — после каждых N роликов блока;
- `every_minutes { minutes }` — не чаще чем раз в N минут;
- `block_start`, `block_end` — в начале и в конце блока;
- `between_blocks` — между блоками;
- `after_category { category }` и `before_category { category }` — после или перед роликом категории.

Действия:

- `jingle { category | items }` — заставка;
- `ad_break { ad_break }` — рекламный блок;
- `promos { count, category }` — анонсы;
- `media { items }` — конкретные ролики.

Условия `conditions`: `categories`, `exclude_categories`, `playlists`, `blocks`, `media`, `exclude_media`, `kinds`, `weekdays`, `from`, `to`.

```toml
[[ad_breaks]]
id = "regular"
category = "commercials"     # или items = ["ad-1", "ad-2"]
order = "least_played"       # listed | random | least_played
max_spots = 3
duration = "00:02:00"        # фиксированная длительность (добор заполнителями и заглушкой)
intro = "jingle-ads-in"
outro = "jingle-ads-out"
fill_category = "fillers"

[ads]
max_per_day = 48             # лимит показов одного ролика в сутки
min_interval = "00:30:00"    # не повторять ролик чаще

[promos]
lookahead = "24:00:00"       # анонсировать только то, что начнётся в этом окне
min_interval = "01:00:00"

[filler]
categories = ["fillers"]
use_promos = true
min_fill = "00:00:03"        # пустоты короче закрываются заглушкой
min_interval = "00:30:00"

[transitions]
fade = 300                   # затемнение в начале и в конце ролика, мс
kinds = ["program"]
```

## graphics.toml

```toml
vars = { ticker = "Добро пожаловать!" }   # переменные шаблонов по умолчанию

[[layers]]
id = 20
name = "Плашки"
exclusive = true             # на слое одновременно показывается один элемент (с наибольшим приоритетом)

[[elements]]
id = "logo"
type = "image"
path = "media/logo.png"
layer = 10
opacity = 0.85
placement = { anchor = "top_right", x = 30, y = 30, width = 160 }

[[elements]]
id = "next-up"
type = "text"
text = "Далее: {next_title} в {next_start}"
layer = 20
placement = { anchor = "bottom_right", x = 60, y = 110 }
animation = { in = "slide_left", out = "fade", duration = "500ms" }
[elements.style]
font = "Arial"               # системный шрифт или путь к .ttf/.otf
size = 28
color = "#FFFFFF"
outline_color = "#000000"
outline_width = 2
shadow_color = "#00000080"
shadow_offset = [2, 2]
background = "#C0182BE0"
padding = 12
corner_radius = 6

[[rules]]
id = "next-up"
element = "next-up"
priority = 5
timing = { last = "20s", before_end = "1s" }   # последние 20 секунд ролика
conditions = { kinds = ["program"] }
```

Типы элементов:

- `image` — изображение;
- `text` — текстовая плашка;
- `ticker` — бегущая строка (`speed` в пикселях в секунду);
- `clock` — часы и дата (`format` в формате strftime, например `"%H:%M"` или `"%d.%m.%Y"`);
- `countdown` — таймер (`target = "item_remaining" | "next_program"`);
- `rect` — фигура (`color`, `width`, `height`, `radius`, оформление в `shape`);
- `plate` — плашка с несколькими строками текста, растущая по тексту (см. ниже);
- `group` — композиция из `children`.

Стиль текста (`style`): `font` — имя системного шрифта или путь к файлу `.ttf`/`.otf` (свои шрифты канала лежат в `channels/<id>/fonts/`), `bold`, `italic`, `size`, `color`, `outline_color`/`outline_width`, `shadow_color`/`shadow_offset`, `background`, `padding`, `corner_radius`.

Плашка:

```toml
[[elements]]
id = "lower-third"
type = "plate"
color = "#101828E0"        # фон (начало градиента)
radius = 8.0
min_width = 520.0          # width = … — фиксированная ширина; без неё — по тексту
padding_x = 24.0
padding_y = 12.0
spacing = 2.0              # между строками
align = "left"             # left | center | right

[elements.shape]           # оформление (так же у rect)
gradient = "#1E3A8AE0"     # второй цвет градиента
gradient_angle = 0.0       # 0 — слева направо, 90 — сверху вниз
border_color = "#FFFFFF"
border_width = 2.0
corners = [16.0, 0.0, 16.0, 0.0]   # углы: лв, пв, пн, лн
skew = -12.0               # наклон, градусы
shadow_color = "#00000080"
shadow_blur = 12.0
shadow_offset = [0.0, 4.0]
image = "media/texture.png"        # картинка внутри фигуры
accent = { side = "left", size = 10.0, color = "#C0182B" }

[[elements.fields]]
label = "Имя"              # для оператора
text = "{presenter}"
style = { size = 40.0, bold = true }

[[elements.fields]]
label = "Должность"
text = "{title}"
style = { size = 24.0, color = "#FFD166" }
```

Пустая после подстановки строка не показывается, плашка становится ниже.

Привязка `anchor`: `top_left`, `top_center`, `top_right`, `center_left`, `center`, `center_right`, `bottom_left`, `bottom_center`, `bottom_right`.

Анимации: `none`, `fade`, `slide_left`, `slide_right`, `slide_up`, `slide_down`.

Параметры показа `timing`:

- `at = ["12:00:00"]` + `duration` — в заданное время суток;
- `after_start` — через N после начала ролика;
- `duration` — сколько показывать;
- `before_end` — скрыть за N до конца;
- `last` — показывать только последние N секунд;
- `every` — повторять с периодом.

Свой текст правила: одна плашка-шаблон может показывать разный текст в разных правилах. `texts` задаёт строки плашки по порядку (у `text` и `ticker` — первая строка); пустая строка оставляет текст из дизайна элемента. `hide_if_empty = true` — не показывать элемент, если весь текст после подстановки переменных пустой (например, `{presenter}` у ролика без ведущего).

```toml
[[rules]]
id = "presenter"
element = "lower-third"
texts = ["{presenter}", "ведущая программы «{block}»"]
hide_if_empty = true
timing = { after_start = "00:00:05", duration = "00:00:10" }
conditions = { categories = ["news"] }
```

`profiles = ["evening"]` у элемента или правила ограничивает его профилем оформления.

Переменные в текстах:

- `{title}`, `{category}`, `{block}`, `{channel}`;
- `{next_title}`, `{next_start}`, `{next_program}`, `{next_program_title}`, `{next_program_start}`;
- поля `meta` ролика и блока, например `{presenter}`;
- переменные из `vars` и заданные оператором через `ctl var` или API.
