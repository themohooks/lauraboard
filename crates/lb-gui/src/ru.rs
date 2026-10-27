//! Russian text for messages the server sends in English (command results,
//! errors, plan warnings). The engine keeps English for logs and the CLI;
//! unknown messages pass through unchanged.

/// `(server pattern, Russian template)`; `{}` in a pattern captures text,
/// `{0}`, `{1}` in the template put it back.
const TABLE: &[(&str, &str)] = &[
    // errors
    ("channel is not broadcasting", "Канал не в эфире — сначала запустите вещание («Запустить» на экране «Эфир»)"),
    ("nothing on air", "Сейчас в эфире ничего не идёт"),
    ("this item cannot be rewound", "Этот элемент нельзя перемотать (поток, картинка или заглушка)"),
    ("could not continue the item at the new position", "Не удалось продолжить ролик с новой позиции — эфир перешёл к следующему элементу"),
    ("unknown media '{}'", "Ролика «{0}» нет в медиатеке"),
    (
        "ad break '{}' unknown or has no eligible spots",
        "В рекламном блоке «{0}» нет роликов, которые можно показать сейчас (или блок удалён)",
    ),
    ("unknown graphic element '{}'", "Элемент графики «{0}» не найден — сохраните графику"),
    ("unknown schedule '{}'", "Расписание «{0}» не найдено"),
    ("channel '{}' is already running in another process", "Канал «{0}» уже запущен в другом процессе"),
    ("channel '{}' not found", "Канал «{0}» не найден"),
    ("channel '{}' already exists", "Канал «{0}» уже есть"),
    ("channel id '{}': use latin letters, digits, '-' and '_'", "Идентификатор «{0}»: допустимы латиница, цифры, «-» и «_»"),
    ("{} already exists (channel disabled?)", "Папка {0} уже существует (возможно, канал выключен)"),
    ("not a LauraBoard channel archive (no manifest.json)", "Это не архив канала LauraBoard (нет manifest.json)"),
    ("not a LauraBoard channel archive", "Это не архив канала LauraBoard"),
    ("{}: not a ZIP archive ({})", "{0}: это не ZIP-архив"),
    (
        "archive format {} is newer than this program supports ({})",
        "Архив сделан более новой версией программы (формат {0}, поддерживается {1})",
    ),
    ("archive path is empty", "Не указан файл архива"),
    ("no media files found", "Медиафайлы не найдены"),
    ("cancelled", "Отменено"),
    ("stop the broadcast before changing the channel id", "Остановите эфир канала, чтобы сменить идентификатор"),
    ("configuration has errors: {}", "В настройках есть ошибки: {0}"),
    ("channel task stopped", "Канал остановлен"),
    ("not found: {}", "Не найдено: {0}"),
    ("failed to parse {}: {}", "Не удалось прочитать {0}: {1}"),
    ("failed to serialize {}: {}", "Не удалось записать {0}: {1}"),
    ("I/O error at {}: {}", "Ошибка доступа к файлу {0}: {1}"),
    ("i/o: {}", "Ошибка ввода-вывода: {0}"),
    ("missing or invalid token", "Нет доступа: неверный или отсутствующий токен API"),
    ("requires {} role", "Недостаточно прав: нужна роль {0}"),
    ("shutting down", "Сервер завершает работу"),
    ("media file not found", "Файл ролика не найден на диске"),
    ("media '{}' is used in playlists: {}", "Ролик «{0}» используется в плейлистах: {1}"),
    ("no preview frame yet", "Кадра предпросмотра пока нет"),
    ("no video in this media", "В этом файле нет видео"),
    ("could not extract a frame", "Не удалось получить кадр из файла"),
    ("no thumbnails for streams", "Для потоков миниатюр нет"),
    ("render failed", "Не удалось отрисовать кадр"),
    ("unsupported frame size {}", "Неподдерживаемый размер кадра {0}"),
    ("output '{}' unreachable: {}", "Выход «{0}» не подключается ({1}) — повтор каждые 5 с, остальные выходы и предпросмотр работают"),
    ("invalid clock format '{}'", "Неверный формат часов «{0}» (например, %H:%M или %d.%m.%Y)"),
    // downloads from sites (yt-dlp)
    ("yt-dlp is not installed", "yt-dlp не установлен на сервере"),
    ("nothing to download", "Нечего скачивать"),
    ("not a web link (http:// or https://)", "Это не ссылка на сайт (нужна http:// или https://)"),
    ("not a web link: {}", "Это не ссылка на сайт: {0}"),
    ("unknown quality '{}'", "Неизвестное качество «{0}»"),
    ("the site did not answer in time", "Сайт не ответил вовремя — попробуйте ещё раз"),
    ("the playlist has no videos", "В плейлисте нет видео"),
    ("yt-dlp failed", "yt-dlp не смог скачать ролик"),
    ("yt-dlp did not report the file", "yt-dlp не сообщил, куда сохранил файл"),
    ("no official yt-dlp build for this platform", "Для этой системы нет готовой сборки yt-dlp — установите его вручную"),
    ("the downloaded yt-dlp does not start", "Скачанный yt-dlp не запускается"),
    ("{}: checksum mismatch (download damaged or replaced)", "{0}: контрольная сумма не совпала — файл повреждён или подменён"),
    ("{}: no checksum in SHA2-256SUMS", "{0}: нет контрольной суммы в SHA2-256SUMS"),
    // command results
    ("started", "Вещание запущено"),
    ("stopped", "Вещание остановлено"),
    ("closing broadcast", "Проигрывается завершение эфира; затем выход отключится"),
    ("skipped", "Переход к следующему элементу"),
    ("restarted", "Текущий элемент запущен сначала"),
    ("position {}", "Ролик перемотан на {0}"),
    ("manual mode", "Ручной режим: расписание на паузе"),
    ("automatic mode", "Эфир снова идёт по расписанию"),
    ("playing", "Ролик выведен в эфир"),
    ("queued ({} in queue)", "Добавлено в очередь (в очереди: {0})"),
    ("queue cleared", "Очередь оператора очищена"),
    ("ad break with {} items inserted", "Рекламный блок поставлен в эфир, роликов: {0}"),
    ("{}: Show", "«{0}» показан в эфире"),
    ("{}: Hide", "«{0}» скрыт в эфире"),
    ("{}: automatic", "«{0}» снова показывается по правилам"),
    ("variable '{}' updated", "Переменная {{0}} обновлена"),
    ("schedule switched", "Расписание переключено"),
    ("reloaded (encoder restarted)", "Настройки применены, кодировщик перезапущен"),
    ("reloaded", "Настройки применены"),
    // plan warnings
    ("'{}' truncated by {}", "«{0}» обрезан на {1}"),
    ("'{}' truncated to fit {}", "«{0}» обрезан, чтобы успеть к {1}"),
    ("ad break '{}' has no eligible spots", "В рекламном блоке «{0}» нет подходящих роликов"),
    ("block '{}' bookend '{}' not in library", "Блок «{0}»: заставки «{1}» нет в медиатеке"),
    ("block '{}' has no valid source", "Блок «{0}»: не задан источник роликов"),
    ("block at {} starts {} late", "Блок в {0} начнётся с опозданием на {1}"),
    ("dropped optional '{}' to keep {}", "Пропущен необязательный «{0}», чтобы успеть к {1}"),
    ("rule '{}' references unknown ad break '{}'", "Правило «{0}» ссылается на несуществующий рекламный блок «{1}»"),
    ("sequential playlist '{}' not found", "Плейлист «{0}» не найден"),
    ("unfilled gap of {} before {}", "Пустота {0} перед {1}"),
    ("no schedule blocks for the planning period", "На этот период в сетке нет ни одного блока"),
];

/// Russian version of a server message, or the message itself.
pub fn server(msg: &str) -> String {
    let msg = msg.trim();
    TABLE.iter().find_map(|(pat, ru)| capture(pat, msg).map(|caps| fill(ru, &caps))).unwrap_or_else(|| msg.to_string())
}

/// Text in place of each `{}` of `pat`, if `s` has its shape.
fn capture<'a>(pat: &str, s: &'a str) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = pat.split("{}").collect();
    let mut rest = s.strip_prefix(parts[0])?;
    let mut caps = Vec::new();
    for (i, lit) in parts.iter().enumerate().skip(1) {
        let last = i == parts.len() - 1;
        let at = if last {
            // the final literal ends the message
            if !rest.ends_with(lit) {
                return None;
            }
            rest.len() - lit.len()
        } else {
            rest.find(lit)?
        };
        if at == 0 {
            return None;
        }
        caps.push(&rest[..at]);
        rest = &rest[at + lit.len()..];
    }
    (rest.is_empty()).then_some(caps)
}

/// `{0}`, `{1}`… → captures (so "{{0}}" gives the capture in braces).
fn fill(template: &str, caps: &[&str]) -> String {
    let mut out = template.to_string();
    for (i, c) in caps.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::server;

    #[test]
    fn translates_known_messages() {
        assert_eq!(server("channel is not broadcasting"), "Канал не в эфире — сначала запустите вещание («Запустить» на экране «Эфир»)");
        assert_eq!(server("ad break with 3 items inserted"), "Рекламный блок поставлен в эфир, роликов: 3");
        assert_eq!(server("dropped optional 'jingle 2' to keep 23:00:00"), "Пропущен необязательный «jingle 2», чтобы успеть к 23:00:00");
        assert_eq!(server("block 'a' bookend 'b' not in library"), "Блок «a»: заставки «b» нет в медиатеке");
        assert_eq!(server("logo: Show"), "«logo» показан в эфире");
        assert_eq!(server("variable 'ticker' updated"), "Переменная {ticker} обновлена");
        assert_eq!(server("reloaded (encoder restarted)"), "Настройки применены, кодировщик перезапущен");
    }

    #[test]
    fn unknown_messages_pass_through() {
        assert_eq!(server("something new"), "something new");
        assert_eq!(server("started later"), "started later");
    }
}
