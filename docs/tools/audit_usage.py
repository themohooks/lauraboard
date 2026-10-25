"""Report real inline image use and task-level navigation (standard library).

Catalog links do not count as illustrated instructions. Run after editing guides:
    python docs/tools/audit_usage.py
This measures documentation, not successful execution of GUI scenarios.
"""
from pathlib import Path
import collections
import json
import re
import unicodedata
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]
IMAGE = re.compile(r'!\[([^\]\n]*)\]\(([^)\n]+)\)')
EXCLUDED = {'coverage.md', 'verification.md', 'image-usage.md', 'screenshots/README.md'}


def slug(title):
    title = re.sub(r'\[([^\]]+)\]\([^)]+\)', r'\1', title).replace('`', '').lower()
    return ''.join(c for c in title if c in '-_ ' or unicodedata.category(c)[0] in 'LN').replace(' ', '-')


def main():
    usage = collections.defaultdict(set)
    tasks = []
    for page in sorted(ROOT.rglob('*.md')):
        rel = page.relative_to(ROOT).as_posix()
        if rel in EXCLUDED or rel.startswith(('audit/', 'screenshots/')):
            continue
        body = page.read_text('utf8')
        for match in IMAGE.finditer(body):
            target = urllib.parse.unquote(match[2].split(' "')[0])
            path = (page.parent / target).resolve()
            if path.is_relative_to(ROOT) and path.suffix.lower() == '.png':
                usage[path.relative_to(ROOT).as_posix()].add(rel)
        if page.parent.name != 'guides' or page.name == 'index.md':
            continue
        heads = list(re.finditer(r'^## (.+)$', body, re.M))
        counts = collections.Counter()
        for i, head in enumerate(heads):
            title = head[1].strip()
            if title == 'Подробные сценарии':
                continue
            anchor = slug(title)
            num = counts[anchor]; counts[anchor] += 1
            if num:
                anchor += f'-{num}'
            section = body[head.end():heads[i+1].start() if i+1 < len(heads) else len(body)]
            images = []
            for match in IMAGE.finditer(section):
                path = (page.parent / urllib.parse.unquote(match[2])).resolve()
                if path.is_relative_to(ROOT):
                    images.append(path.relative_to(ROOT).as_posix())
            tasks.append({'task':title, 'guide':rel, 'anchor':anchor, 'images':images,
                          'status':'Частично: код и снимок; полный GUI-проход не подтверждён' if images else
                                   'Частично: инструкция; отдельный кадр этого раздела отсутствует'})
    pngs = sorted(p.relative_to(ROOT).as_posix() for p in (ROOT/'screenshots').rglob('*.png'))
    rows = []
    cropped_sources = {
        'encoder-bottom': 'encoder-frame-sound',
        'media-transform': 'media-transforms',
        'plate-layout-complete': 'plate-layout',
        'plate-layout': 'plate-shape-start',
        'plate-shape-bottom': 'plate-shape-end',
        'recording-storage': 'recording-codecs-storage',
    }
    for path in pngs:
        base = re.sub(r'-(light|dark)$', '', Path(path).stem)
        if usage[path]:
            role = 'Встроен в инструкцию'
        elif path.endswith('-dark.png') and usage[path.replace('-dark.png','-light.png')]:
            role = 'Архив: альтернативная тёмная тема используемого кадра'
        elif path.endswith('-light.png') and usage[path.replace('-light.png','-dark.png')]:
            role = 'Архив: альтернативная светлая тема используемого кадра'
        elif base in cropped_sources and usage['screenshots/parts/'+cropped_sources[base]+'.png']:
            role = 'Исходник фрагмента '+cropped_sources[base]+'.png'+(' (тёмный вариант)' if path.endswith('-dark.png') else '')
        elif base == 'scene-music':
            role = 'Ранний общий вид; в гайде использован более полный scene-music-options и его фрагменты'
        else:
            role = 'Архив: исходник/вариант, отдельно в инструкции не встроен'
        rows.append({'image':path, 'pages':sorted(usage[path]), 'role':role})
    data = {'counting':'Only inline Markdown images outside catalogs, audit and verification pages',
            'png_total':len(pngs), 'png_embedded':sum(bool(usage[p]) for p in pngs),
            'inline_occurrences_unique_per_page':sum(len(usage[p]) for p in pngs),
            'images':rows, 'guide_sections':tasks}
    (ROOT/'audit/image-usage.json').write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
    text = ['# Использование скриншотов в инструкциях', '',
            'Считаются только изображения, встроенные в пользовательские страницы через синтаксис Markdown. Ссылки каталога, реестра покрытия и отчёта не увеличивают результат. Этот счётчик не доказывает полноту функций или выполнение действий.', '',
            f'Встроено **{data["png_embedded"]} из {data["png_total"]} PNG**. Остальные перечислены с причиной хранения. Дубли темы и исходники фрагментов не требуют повторного размещения ради счётчика.', '',
            '[Машинный отчёт](audit/image-usage.json) · [Скрипт](tools/audit_usage.py) · [Задачи и пробелы](coverage.md) · [Каталог гайдов](guides/index.md)', '',
            '| Изображение | Использование | Страницы с изображением |', '|---|---|---|']
    for row in rows:
        pages = ', '.join(f'[{p}]({p})' for p in row['pages']) or '—'
        text.append(f'| [{Path(row["image"]).name}]({row["image"]}) | {row["role"]} | {pages} |')
    (ROOT/'image-usage.md').write_text('\n'.join(text)+'\n',encoding='utf8')
    text = ['# Покрытие пользовательских задач', '',
            'Каждая строка соответствует разделу практического гайда. В столбце снимков — только изображения внутри этого раздела, а не первые картинки всей страницы. «Нет отдельного кадра» оставлено явно: это визуальный пробел либо текстовая/CLI-операция; общий кадр соседнего раздела не выдаётся за её выполнение.', '',
            'Проверка исходного кода и реальный снимок не равны полному прохождению задачи мышью. Для проверок API/CLI, тестов и непроверенных платформ используйте [отчёт](verification.md) и [машинные результаты](audit/runtime-checks.json). Для каждой задачи результат проверяют способом, указанным в её инструкции; ниже указан предел фактической проверки документации.', '',
            '[Каталог гайдов](guides/index.md) · [Использование изображений](image-usage.md) · [Главная](index.md)', '',
            '| Задача | Гайд | Скриншоты этого раздела | Способ проверки | Статус |', '|---|---|---|---|---|']
    for task in tasks:
        imgs = ' / '.join(f'[кадр {i+1}]({p})' for i,p in enumerate(task['images'])) or 'Нет отдельного кадра'
        text.append(f'| {task["task"].replace("|", "/")} | [Открыть шаги]({task["guide"]}#{task["anchor"]}) | {imgs} | По шагам и проверке результата в гайде; фактические испытания — в отчёте | {task["status"]} |')
    (ROOT/'coverage.md').write_text('\n'.join(text)+'\n',encoding='utf8')
    print(f'{len(tasks)} guide sections; {data["png_embedded"]}/{len(pngs)} PNG embedded')


if __name__ == '__main__':
    main()
