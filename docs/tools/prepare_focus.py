"""Crop reviewed, real UI screenshots to the controls discussed in guides.

Requires Pillow. No scaling, redrawing of controls or replacement of text.
Full frames stay in screenshots/interface; crop coordinates are published.
"""
from pathlib import Path
import json
from PIL import Image

ROOT=Path(__file__).resolve().parents[1]
PARTS=[
    # source, box, output, expected size
    ('media-import',(478,337,962,548),'import-path-type-folder',(1440,960)),
    ('media-import',(478,554,962,788),'import-copy-actions',(1440,960)),
    ('media-multiselect',(494,124,1467,177),'media-bulk-toolbar',(1920,1080)),
    ('playlists',(548,65,1427,231),'playlist-name-modes',(1440,960)),
    ('schedule',(246,65,1068,184),'schedule-choice-days',(1440,960)),
    ('stream-dialog',(478,410,962,680),'stream-name-url-duration',(1440,960)),
    ('air',(260,405,975,497),'air-position-times',(1440,960)),
    ('air',(248,602,988,675),'air-transport',(1440,960)),
    ('air',(1007,192,1424,385),'air-queue-columns',(1440,960)),
    ('log',(246,130,1424,180),'log-search-levels',(1440,960)),
    ('ad-breaks',(563,227,1395,386),'ads-source-order',(1440,960)),
    ('ad-breaks',(563,399,1395,532),'ads-duration-fill',(1440,960)),
    ('ad-breaks',(563,544,1395,750),'ads-intro-usage',(1440,960)),
    ('rule-conditions-complete',(695,185,1388,446),'rule-content-filters',(1440,960)),
    ('rule-conditions-complete',(695,451,1388,582),'rule-week-time',(1440,960)),
    ('rule-conditions-complete',(695,610,1388,882),'rule-priority-options',(1440,960)),
    ('scene-music-options',(644,202,1382,423),'music-track-sources',(1440,960)),
    ('scene-music-options',(644,426,1382,588),'music-volume-repeat',(1440,960)),
    ('scene-music-options',(644,599,1382,690),'music-days-hours',(1440,960)),
    ('element-image-top',(1524,242,1874,451),'gfx-position',(1920,1080)),
    ('element-image-top',(1524,493,1874,611),'gfx-image-file',(1920,1080)),
    ('element-group-top',(1524,493,1874,584),'gfx-group-children',(1920,1080)),
    ('element-avatar-top',(1524,493,1874,611),'gfx-avatar-file',(1920,1080)),
    ('element-image-top',(1524,635,1874,894),'gfx-image-animation-profile',(1920,1080)),
    ('element-text-top',(1524,493,1874,643),'gfx-text-content',(1920,1080)),
    ('element-text-style',(1524,387,1874,744),'gfx-text-font-outline',(1920,1080)),
    ('element-text-style',(1524,747,1874,943),'gfx-text-background',(1920,1080)),
    ('graphic-animation',(1524,781,1874,1048),'gfx-animation-profile',(1920,1080)),
    ('clock-properties',(1524,310,1874,445),'gfx-clock-format',(1920,1080)),
    ('countdown-properties',(1524,310,1874,459),'gfx-countdown-target',(1920,1080)),
    ('program-time-properties',(1524,310,1874,650),'gfx-program-exclusions',(1920,1080)),
    ('ticker-properties',(1524,310,1874,532),'gfx-ticker-speed-text',(1920,1080)),
    ('progress-properties',(1524,310,1874,639),'gfx-progress-dimensions',(1920,1080)),
    ('progress-properties',(1524,645,1874,816),'gfx-progress-exclusions',(1920,1080)),
    ('progress-appearance',(1524,463,1874,1035),'gfx-progress-appearance',(1920,1080)),
    ('element-shape-top',(1524,493,1874,650),'gfx-shape-size',(1920,1080)),
    ('element-shape-design',(1524,280,1874,518),'gfx-shape-gradient-border',(1920,1080)),
    ('element-shape-design',(1524,524,1874,638),'gfx-shape-shadow',(1920,1080)),
    ('element-shape-end',(1524,486,1874,750),'gfx-shape-accent-texture',(1920,1080)),
    ('element-shape-end',(1524,784,1874,1046),'gfx-shape-animation',(1920,1080)),
    ('element-plate-top',(1524,614,1874,927),'gfx-plate-dimensions',(1920,1080)),
    ('element-plate-lines',(1524,433,1874,850),'gfx-plate-lines',(1920,1080)),
    ('element-plate-design',(1524,532,1874,1020),'gfx-plate-design',(1920,1080)),
    ('element-plate-end',(1524,184,1874,742),'gfx-plate-line-style',(1920,1080)),
    ('element-plate-top',(375,582,1050,735),'gfx-plate-result',(1920,1080)),
]

def main():
    out=ROOT/'screenshots/parts';out.mkdir(exist_ok=True)
    records=[]
    for source,box,name,size in PARTS:
        path=ROOT/'screenshots/interface'/(source+'-light.png')
        with Image.open(path) as im:
            if im.size!=size:raise ValueError(f'{source}: expected {size}, got {im.size}')
            x1,y1,x2,y2=box
            if not (0<=x1<x2<=size[0] and 0<=y1<y2<=size[1]):raise ValueError(name)
            im.crop(box).save(out/(name+'.png'),optimize=True)
        records.append({'source':path.relative_to(ROOT).as_posix(),'box':box,'output':f'screenshots/parts/{name}.png','method':'unchanged pixel crop, no scaling or control edits'})
    (ROOT/'screenshots/focus-manifest.json').write_text(json.dumps(records,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
    print(f'{len(records)} focused screenshot fragments written')

if __name__=='__main__':main()
