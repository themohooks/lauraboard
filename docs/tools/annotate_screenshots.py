"""Add tutorial callouts to original Laurabrod captures (Pillow required).

Run from anywhere: python docs/tools/annotate_screenshots.py
Original PNGs are never modified. Coordinates target the reviewed 1440x960
captures, so changing their size requires reviewing this annotation map.
"""
from pathlib import Path
import math
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'screenshots/interface'
DEST = ROOT / 'screenshots/guides'
FONT = Path(__file__).resolve().parents[2] / 'crates/lb-gui/assets/fonts/Roboto.ttf'

# (input, output, title, [(rectangle, numbered label, badge centre)])
SHOTS = [
 ('home', 'channel-01', 'Создание канала · 1. Открыть менеджер', [
     ((436,8,478,53), 'Кнопка управления каналами в шапке окна', (505,78))]),
 ('channels', 'channel-02', 'Создание канала · 2. Выбрать новое создание', [
     ((830,645,985,690), 'Нажмите «Новый канал»', (1025,735))]),
 ('new-channel', 'channel-03', 'Создание канала · 3. Заполнить форму', [
     ((523,429,917,479), 'Идентификатор: латиница, цифры, дефис или подчёркивание', (478,451)),
     ((523,512,917,562), 'Название: понятное имя вашего канала', (478,537)),
     ((812,641,919,683), '«Создать» станет доступно после корректного ввода', (964,701))]),
 ('library', 'import-01', 'Импорт · 1. Открыть форму', [
     ((1305,126,1423,169), 'Импорт файлов, папки или URL', (1265,207))]),
 ('media-import', 'import-02', 'Импорт · 2. Выбрать данные и способ хранения', [
     ((483,347,861,398), 'Путь на сервере; несколько путей разделяются точкой с запятой', (441,373)),
     ((483,430,955,480), 'Тип материала и категория для правил', (1015,454)),
     ((483,560,800,599), 'Копирование делает канал независимым от исходной папки', (441,579)),
     ((803,738,958,782), 'После заполнения нажмите «Импортировать»', (1015,758))]),
 ('schedule', 'schedule-01', 'Сетка · 1. Выбрать день и добавить блок', [
     ((248,67,489,125), 'Сначала выберите нужное расписание', (552,54)),
     ((248,130,1030,188), 'Выберите шаблон и дни его применения', (1082,193)),
     ((1068,616,1415,660), 'Блок по времени или блок следом', (1009,650))]),
 ('schedule-properties', 'schedule-02', 'Сетка · 2. Уточнить поведение блока', [
     ((1103,153,1407,272), 'Политика опоздания и отдельное разрешение обрезки', (1038,197)),
     ((1103,655,1407,761), 'Включение правил вставок и заполнения пустот', (1038,715))]),
 ('jingles', 'rules-01', 'Правила · 1. Начать создание', [
     ((487,147,657,189), 'Мастер нового правила', (579,111)),
     ((699,271,1386,320), 'В редакторе здесь выбирается момент вставки', (660,303))]),
 ('rule-wizard', 'rules-02', 'Правила · 2. Задать событие', [
     ((363,384,1077,437), 'Название правила', (310,410)),
     ((379,497,1062,548), 'Триггер: когда проверять вставку', (1125,518)),
     ((985,617,1078,661), 'Перейти к выбору содержимого', (1130,674))]),
 ('jingles', 'rules-03', 'Правила · 3. Выбрать содержимое и условия', [
     ((699,389,1386,448), 'Действие и источник заставок', (655,409)),
     ((1163,474,1385,515), 'Явно выбрать материалы', (1130,541)),
     ((699,647,1386,762), 'Ограничить категории, блоки и плейлисты', (655,674))]),
 ('recording', 'recording-01', 'Запись · 1. Файлы и включение', [
     ((383,229,610,269), 'Разрешить запись эфира', (331,249)),
     ((383,317,1033,449), 'Контейнер, имя и папка на сервере', (1103,377)),
     ((383,475,1033,531), 'Длительность в минутах и предельный размер в МБ', (331,502)),
     ((919,826,1056,869), 'Сохранить настройки записи', (1110,850))]),
 ('plate-editor', 'plate-01', 'Плашка · 1. Проверить стиль и текст', [
     ((153,171,675,391), 'Точный рендер текущей плашки', (103,245)),
     ((153,486,675,580), 'Готовые стили меняют оформление, сохраняя текст', (103,518)),
     ((704,217,1248,327), 'Выбор шрифта, размера и начертания строки', (1315,258)),
     ((1191,794,1287,837), 'Закрыть редактор, затем сохранить всю графику', (1315,863))]),
 ('recording-storage', 'recording-02', 'Запись · 2. Кодеки и хранение', [
     ((383,341,1033,478), 'Отдельное кодирование изображения и выбор качества', (331,405)),
     ((383,534,1033,682), 'Кодек, частота и каналы звука записи', (1103,595)),
     ((383,730,1033,790), 'Лимиты хранения могут удалять старые файлы', (331,761))]),
 ('scene-music-options', 'music-01', 'Музыка сцены · Порядок, повтор и расписание', [
     ((650,428,1371,479), 'Порядок, громкость, нарастание и затухание', (594,456)),
     ((650,492,1371,541), 'Повторы, пауза между треками и задержка начала', (1420,520)),
     ((650,626,1371,678), 'Дни недели и часы действия музыкального правила', (594,650))]),
 ('rules-limits', 'rules-04', 'Правила · Приоритет и проверка результата', [
     ((1072,515,1386,567), 'Приоритет и группа разрешают конкуренцию правил', (1420,543)),
     ((699,760,1130,803), 'План — прогноз; журнал срабатываний — фактическая история', (650,782))]),
]

def font(size):
    return ImageFont.truetype(str(FONT), size)

def arrow(draw, start, end, color):
    draw.line([start, end], fill='white', width=9)
    draw.line([start, end], fill=color, width=5)
    angle=math.atan2(end[1]-start[1], end[0]-start[0])
    points=[end, (end[0]-18*math.cos(angle-.45),end[1]-18*math.sin(angle-.45)),
            (end[0]-18*math.cos(angle+.45),end[1]-18*math.sin(angle+.45))]
    draw.polygon(points,fill=color)

def render(source, name, title, marks):
    raw=Image.open(SOURCE/(source+'-light.png')).convert('RGB')
    if raw.size!=(1440,960):
        raise ValueError(f'{source}: expected 1440x960, got {raw.size}')
    footer=90+len(marks)*37
    output=Image.new('RGB',(raw.width,raw.height+footer),'#F4F6FB')
    output.paste(raw,(0,0))
    draw=ImageDraw.Draw(output)
    color='#C81E32'
    for number,(box,label,badge) in enumerate(marks,1):
        x0,y0,x1,y1=box
        # Keep borders outside field labels and original control text.
        outline=(x0-7,y0-13,x1+7,y1+7)
        draw.rounded_rectangle(outline,radius=8,outline='white',width=8)
        draw.rounded_rectangle(outline,radius=8,outline=color,width=4)
        # Point to the nearest edge, leaving original field text untouched.
        end=(min(max(badge[0],x0),x1),min(max(badge[1],y0),y1))
        arrow(draw,badge,end,color)
        x,y=badge
        draw.ellipse((x-19,y-19,x+19,y+19),fill=color,outline='white',width=3)
        draw.text((x,y-1),str(number),fill='white',font=font(22),anchor='mm')
    draw.line([(0,960),(1440,960)],fill='#415B88',width=3)
    draw.text((24,979),title,fill='#142742',font=font(25))
    for number,(_,label,_) in enumerate(marks,1):
        draw.text((26,1020+(number-1)*37),f'{number}. {label}',fill='#142742',font=font(20))
    DEST.mkdir(parents=True,exist_ok=True)
    output.save(DEST/(name+'.png'),optimize=True)

if __name__=='__main__':
    for args in SHOTS:
        render(*args)
    print(f'Annotated {len(SHOTS)} screenshots; originals preserved')
