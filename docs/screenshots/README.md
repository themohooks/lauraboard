# Снимки интерфейса Laurabrod

Все изображения показывают настоящий интерфейс программы 0.1.0. Элементы программы не дорисовывались; в `guides/` добавлены только поясняющие рамки, стрелки, номера и подписи. На системных снимках явно скрыто имя компьютера; исходные частные кадры не публикуются. [Как читать гайды](../guides/index.md).

## Происхождение

- `interface/`: Slint-рендер настоящих экранов. Большинство подключено к действительному учебному движку; это не генеративные изображения и не нарисованные макеты.
- `channel-export`, `channel-import`, `jobs`, `welcome`: встроенные демонстрационные состояния реальных окон с вымышленными данными.
- `plate-editor` и `graphics-font`: текущая плашка учебного канала; список собственных стилей заполнен встроенными примерами.
- `installation/`: встроенные демонстрационные страницы реального установщика; установка и удаление не выполнялись.
- `guides/`: производные исходных светлых снимков с обводками и пояснениями. Исходники остаются в `interface/`. [Скрипт аннотаций](../tools/annotate_screenshots.py).
- `parts/`: логические фрагменты длинных форм. [Скрипт подготовки](../tools/prepare_parts.py). Кадры с частным путём профиля остаются вне документации; опубликованные фрагменты его не включают.
- Длинные формы разделены на части: настройки файла, изображение, звук, хранение и т. п. В подписи к гайдовой иллюстрации указано, какую часть смотреть.

Headless-режим не создаёт системный трей; недоступный переключатель на его снимке не описывает возможность обычного запуска Windows. Снимок не доказывает прохождение всех действий формы. [Пределы проверки](../verification.md). [Манифест захвата](capture-manifest.json) фиксирует режимы, разделы и параметры съёмки; средство изолированной копии дополнено только прокруткой и отключением всплывающих уведомлений при захвате.

## Дополнительный захват подробных свойств

Часы, отсчёт, прогресс, фигура, бегущая строка и анимации сняты в учебном канале отдельными состояниями панели свойств (1920×1080). Положение и содержимое графики подготовлены через API; панели открыты реальным интерфейсом. `playlist-selection` показывает выделение строки, а не открытие свойств. `media-multiselect` показывает выделение нескольких записей. Запись при этом выключена; использовался локальный эфир. Для длинных свойств блока сетки добавлены верхний и нижний фрагменты одного полного кадра.

[Какие изображения встроены в гайды](../image-usage.md). Тёмные варианты сохранены как альтернативная тема; их повторное размещение не требуется для описания тех же полей.

## Каталог исходников и иллюстраций

| Изображение | Файл |
|---|---|
| channel-01 | [PNG](guides/channel-01.png) |
| channel-02 | [PNG](guides/channel-02.png) |
| channel-03 | [PNG](guides/channel-03.png) |
| import-01 | [PNG](guides/import-01.png) |
| import-02 | [PNG](guides/import-02.png) |
| music-01 | [PNG](guides/music-01.png) |
| plate-01 | [PNG](guides/plate-01.png) |
| recording-01 | [PNG](guides/recording-01.png) |
| recording-02 | [PNG](guides/recording-02.png) |
| rules-01 | [PNG](guides/rules-01.png) |
| rules-02 | [PNG](guides/rules-02.png) |
| rules-03 | [PNG](guides/rules-03.png) |
| rules-04 | [PNG](guides/rules-04.png) |
| schedule-01 | [PNG](guides/schedule-01.png) |
| schedule-02 | [PNG](guides/schedule-02.png) |
| setup-done-dark | [PNG](installation/setup-done-dark.png) |
| setup-done-light | [PNG](installation/setup-done-light.png) |
| setup-failed-dark | [PNG](installation/setup-failed-dark.png) |
| setup-failed-light | [PNG](installation/setup-failed-light.png) |
| setup-location-dark | [PNG](installation/setup-location-dark.png) |
| setup-location-error-dark | [PNG](installation/setup-location-error-dark.png) |
| setup-location-error-light | [PNG](installation/setup-location-error-light.png) |
| setup-location-light | [PNG](installation/setup-location-light.png) |
| setup-missing-dark | [PNG](installation/setup-missing-dark.png) |
| setup-missing-light | [PNG](installation/setup-missing-light.png) |
| setup-options-dark | [PNG](installation/setup-options-dark.png) |
| setup-options-light | [PNG](installation/setup-options-light.png) |
| setup-progress-dark | [PNG](installation/setup-progress-dark.png) |
| setup-progress-light | [PNG](installation/setup-progress-light.png) |
| setup-uninstall-dark | [PNG](installation/setup-uninstall-dark.png) |
| setup-uninstall-light | [PNG](installation/setup-uninstall-light.png) |
| setup-welcome-dark | [PNG](installation/setup-welcome-dark.png) |
| setup-welcome-light | [PNG](installation/setup-welcome-light.png) |
| setup-welcome-update-dark | [PNG](installation/setup-welcome-update-dark.png) |
| setup-welcome-update-light | [PNG](installation/setup-welcome-update-light.png) |
| ad-breaks-dark | [PNG](interface/ad-breaks-dark.png) |
| ad-breaks-light | [PNG](interface/ad-breaks-light.png) |
| ad-policies-bottom-dark | [PNG](interface/ad-policies-bottom-dark.png) |
| ad-policies-bottom-light | [PNG](interface/ad-policies-bottom-light.png) |
| ad-policies-dark | [PNG](interface/ad-policies-dark.png) |
| ad-policies-light | [PNG](interface/ad-policies-light.png) |
| ads-dark | [PNG](interface/ads-dark.png) |
| ads-light | [PNG](interface/ads-light.png) |
| air-dark | [PNG](interface/air-dark.png) |
| air-light | [PNG](interface/air-light.png) |
| channel-delete-dark | [PNG](interface/channel-delete-dark.png) |
| channel-delete-light | [PNG](interface/channel-delete-light.png) |
| channel-export-dark | [PNG](interface/channel-export-dark.png) |
| channel-export-light | [PNG](interface/channel-export-light.png) |
| channel-import-dark | [PNG](interface/channel-import-dark.png) |
| channel-import-light | [PNG](interface/channel-import-light.png) |
| channel-rename-dark | [PNG](interface/channel-rename-dark.png) |
| channel-rename-light | [PNG](interface/channel-rename-light.png) |
| channels-dark | [PNG](interface/channels-dark.png) |
| channels-light | [PNG](interface/channels-light.png) |
| clock-properties-dark | [PNG](interface/clock-properties-dark.png) |
| clock-properties-light | [PNG](interface/clock-properties-light.png) |
| countdown-properties-dark | [PNG](interface/countdown-properties-dark.png) |
| countdown-properties-light | [PNG](interface/countdown-properties-light.png) |
| encoder-bottom-dark | [PNG](interface/encoder-bottom-dark.png) |
| encoder-bottom-light | [PNG](interface/encoder-bottom-light.png) |
| encoder-dark | [PNG](interface/encoder-dark.png) |
| encoder-light | [PNG](interface/encoder-light.png) |
| encoder-middle-dark | [PNG](interface/encoder-middle-dark.png) |
| encoder-middle-light | [PNG](interface/encoder-middle-light.png) |
| graphic-animation-dark | [PNG](interface/graphic-animation-dark.png) |
| graphic-animation-light | [PNG](interface/graphic-animation-light.png) |
| graphics-add-dark | [PNG](interface/graphics-add-dark.png) |
| graphics-add-light | [PNG](interface/graphics-add-light.png) |
| graphics-dark | [PNG](interface/graphics-dark.png) |
| graphics-font-dark | [PNG](interface/graphics-font-dark.png) |
| graphics-font-light | [PNG](interface/graphics-font-light.png) |
| graphics-light | [PNG](interface/graphics-light.png) |
| graphics-rules-dark | [PNG](interface/graphics-rules-dark.png) |
| graphics-rules-light | [PNG](interface/graphics-rules-light.png) |
| graphics-templates-dark | [PNG](interface/graphics-templates-dark.png) |
| graphics-templates-light | [PNG](interface/graphics-templates-light.png) |
| graphics-text-properties-dark | [PNG](interface/graphics-text-properties-dark.png) |
| graphics-text-properties-light | [PNG](interface/graphics-text-properties-light.png) |
| graphics-vars-dark | [PNG](interface/graphics-vars-dark.png) |
| graphics-vars-light | [PNG](interface/graphics-vars-light.png) |
| home-dark | [PNG](interface/home-dark.png) |
| home-light | [PNG](interface/home-light.png) |
| jingles-dark | [PNG](interface/jingles-dark.png) |
| jingles-light | [PNG](interface/jingles-light.png) |
| jobs-dark | [PNG](interface/jobs-dark.png) |
| jobs-light | [PNG](interface/jobs-light.png) |
| library-dark | [PNG](interface/library-dark.png) |
| library-light | [PNG](interface/library-light.png) |
| log-dark | [PNG](interface/log-dark.png) |
| log-light | [PNG](interface/log-light.png) |
| log-server-dark | [PNG](interface/log-server-dark.png) |
| log-server-light | [PNG](interface/log-server-light.png) |
| media-bottom-dark | [PNG](interface/media-bottom-dark.png) |
| media-bottom-light | [PNG](interface/media-bottom-light.png) |
| media-colour-dark | [PNG](interface/media-colour-dark.png) |
| media-colour-light | [PNG](interface/media-colour-light.png) |
| media-display-dark | [PNG](interface/media-display-dark.png) |
| media-display-light | [PNG](interface/media-display-light.png) |
| media-edit-dark | [PNG](interface/media-edit-dark.png) |
| media-edit-light | [PNG](interface/media-edit-light.png) |
| media-folder-dark | [PNG](interface/media-folder-dark.png) |
| media-folder-light | [PNG](interface/media-folder-light.png) |
| media-import-dark | [PNG](interface/media-import-dark.png) |
| media-import-light | [PNG](interface/media-import-light.png) |
| media-multiselect-dark | [PNG](interface/media-multiselect-dark.png) |
| media-multiselect-light | [PNG](interface/media-multiselect-light.png) |
| media-transform-dark | [PNG](interface/media-transform-dark.png) |
| media-transform-light | [PNG](interface/media-transform-light.png) |
| new-channel-dark | [PNG](interface/new-channel-dark.png) |
| new-channel-light | [PNG](interface/new-channel-light.png) |
| new-playlist-dark | [PNG](interface/new-playlist-dark.png) |
| new-playlist-light | [PNG](interface/new-playlist-light.png) |
| output-form-dark | [PNG](interface/output-form-dark.png) |
| output-form-light | [PNG](interface/output-form-light.png) |
| plate-editor-dark | [PNG](interface/plate-editor-dark.png) |
| plate-editor-light | [PNG](interface/plate-editor-light.png) |
| plate-layout-complete-dark | [PNG](interface/plate-layout-complete-dark.png) |
| plate-layout-complete-light | [PNG](interface/plate-layout-complete-light.png) |
| plate-layout-dark | [PNG](interface/plate-layout-dark.png) |
| plate-layout-light | [PNG](interface/plate-layout-light.png) |
| plate-shape-bottom-dark | [PNG](interface/plate-shape-bottom-dark.png) |
| plate-shape-bottom-light | [PNG](interface/plate-shape-bottom-light.png) |
| playlist-add-dark | [PNG](interface/playlist-add-dark.png) |
| playlist-add-light | [PNG](interface/playlist-add-light.png) |
| playlist-selection-dark | [PNG](interface/playlist-selection-dark.png) |
| playlist-selection-light | [PNG](interface/playlist-selection-light.png) |
| playlists-dark | [PNG](interface/playlists-dark.png) |
| playlists-light | [PNG](interface/playlists-light.png) |
| program-time-properties-dark | [PNG](interface/program-time-properties-dark.png) |
| program-time-properties-light | [PNG](interface/program-time-properties-light.png) |
| progress-appearance-dark | [PNG](interface/progress-appearance-dark.png) |
| progress-appearance-light | [PNG](interface/progress-appearance-light.png) |
| progress-properties-dark | [PNG](interface/progress-properties-dark.png) |
| progress-properties-light | [PNG](interface/progress-properties-light.png) |
| recording-dark | [PNG](interface/recording-dark.png) |
| recording-encoding-dark | [PNG](interface/recording-encoding-dark.png) |
| recording-encoding-light | [PNG](interface/recording-encoding-light.png) |
| recording-light | [PNG](interface/recording-light.png) |
| recording-storage-dark | [PNG](interface/recording-storage-dark.png) |
| recording-storage-light | [PNG](interface/recording-storage-light.png) |
| rule-conditions-complete-dark | [PNG](interface/rule-conditions-complete-dark.png) |
| rule-conditions-complete-light | [PNG](interface/rule-conditions-complete-light.png) |
| rule-wizard-conditions-dark | [PNG](interface/rule-wizard-conditions-dark.png) |
| rule-wizard-conditions-light | [PNG](interface/rule-wizard-conditions-light.png) |
| rule-wizard-content-dark | [PNG](interface/rule-wizard-content-dark.png) |
| rule-wizard-content-light | [PNG](interface/rule-wizard-content-light.png) |
| rule-wizard-dark | [PNG](interface/rule-wizard-dark.png) |
| rule-wizard-light | [PNG](interface/rule-wizard-light.png) |
| rule-wizard-limits-dark | [PNG](interface/rule-wizard-limits-dark.png) |
| rule-wizard-limits-light | [PNG](interface/rule-wizard-limits-light.png) |
| rules-limits-dark | [PNG](interface/rules-limits-dark.png) |
| rules-limits-light | [PNG](interface/rules-limits-light.png) |
| scene-gaps-dark | [PNG](interface/scene-gaps-dark.png) |
| scene-gaps-light | [PNG](interface/scene-gaps-light.png) |
| scene-music-dark | [PNG](interface/scene-music-dark.png) |
| scene-music-light | [PNG](interface/scene-music-light.png) |
| scene-music-options-dark | [PNG](interface/scene-music-options-dark.png) |
| scene-music-options-light | [PNG](interface/scene-music-options-light.png) |
| scene-music-timing-dark | [PNG](interface/scene-music-timing-dark.png) |
| scene-music-timing-light | [PNG](interface/scene-music-timing-light.png) |
| scene-rules-dark | [PNG](interface/scene-rules-dark.png) |
| scene-rules-light | [PNG](interface/scene-rules-light.png) |
| scenes-dark | [PNG](interface/scenes-dark.png) |
| scenes-light | [PNG](interface/scenes-light.png) |
| schedule-dark | [PNG](interface/schedule-dark.png) |
| schedule-light | [PNG](interface/schedule-light.png) |
| schedule-properties-dark | [PNG](interface/schedule-properties-dark.png) |
| schedule-properties-light | [PNG](interface/schedule-properties-light.png) |
| settings-dark | [PNG](interface/settings-dark.png) |
| settings-light | [PNG](interface/settings-light.png) |
| shape-properties-dark | [PNG](interface/shape-properties-dark.png) |
| shape-properties-light | [PNG](interface/shape-properties-light.png) |
| sources-dark | [PNG](interface/sources-dark.png) |
| sources-light | [PNG](interface/sources-light.png) |
| stats-all-dark | [PNG](interface/stats-all-dark.png) |
| stats-all-light | [PNG](interface/stats-all-light.png) |
| stats-dark | [PNG](interface/stats-dark.png) |
| stats-light | [PNG](interface/stats-light.png) |
| system-dark | [PNG](interface/system-dark.png) |
| system-light | [PNG](interface/system-light.png) |
| ticker-properties-dark | [PNG](interface/ticker-properties-dark.png) |
| ticker-properties-light | [PNG](interface/ticker-properties-light.png) |
| transitions-dark | [PNG](interface/transitions-dark.png) |
| transitions-light | [PNG](interface/transitions-light.png) |
| welcome-dark | [PNG](interface/welcome-dark.png) |
| welcome-light | [PNG](interface/welcome-light.png) |
| encoder-frame-sound | [PNG](parts/encoder-frame-sound.png) |
| encoder-video-bitrate | [PNG](parts/encoder-video-bitrate.png) |
| media-properties | [PNG](parts/media-properties.png) |
| media-transforms | [PNG](parts/media-transforms.png) |
| plate-layout | [PNG](parts/plate-layout.png) |
| plate-shape-end | [PNG](parts/plate-shape-end.png) |
| plate-shape-start | [PNG](parts/plate-shape-start.png) |
| plate-text | [PNG](parts/plate-text.png) |
| recording-codecs-storage | [PNG](parts/recording-codecs-storage.png) |
| recording-files | [PNG](parts/recording-files.png) |
| scene-graphics | [PNG](parts/scene-graphics.png) |
| scene-music | [PNG](parts/scene-music.png) |
| schedule-block-bottom | [PNG](parts/schedule-block-bottom.png) |
| schedule-block-top | [PNG](parts/schedule-block-top.png) |
| settings-about | [PNG](parts/settings-about.png) |
| settings-connection | [PNG](parts/settings-connection.png) |
| settings-layout | [PNG](parts/settings-layout.png) |
| settings-shortcuts | [PNG](parts/settings-shortcuts.png) |
