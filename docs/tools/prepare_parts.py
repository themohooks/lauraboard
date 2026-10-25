"""Make logical crops of reviewed screenshots; originals are not modified.

Requires Pillow. Private settings frames stay in the ignored audit folder;
only crops that exclude the workstation profile path are published.
"""
from pathlib import Path
from PIL import Image

ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'screenshots/interface'
OUT=ROOT/'screenshots/parts'
PARTS=[
 ('media-edit',(1020,60,1440,601),'media-properties'),
 ('media-transform',(1037,265,1411,892),'media-transforms'),
 ('plate-editor',(687,170,1265,755),'plate-text'),
 ('plate-layout-complete',(687,334,1265,714),'plate-layout'),
 ('plate-layout',(687,499,1265,759),'plate-shape-start'),
 ('plate-shape-bottom',(687,228,1265,759),'plate-shape-end'),
 ('encoder',(376,235,1040,681),'encoder-video-bitrate'),
 ('encoder-bottom',(376,280,1040,791),'encoder-frame-sound'),
 ('scene-music-options',(624,137,1400,760),'scene-music'),
 ('scene-music-options',(624,769,1400,944),'scene-graphics'),
 ('recording',(376,281,1040,548),'recording-files'),
 ('recording-storage',(376,281,1040,791),'recording-codecs-storage'),
]

def crop(source,box,name):
    with Image.open(source) as im:
        if im.size!=(1440,960):raise ValueError(f'Review coordinates for {source}: {im.size}')
        im.crop(box).save(OUT/(name+'.png'),optimize=True)

if __name__=='__main__':
    OUT.mkdir(parents=True,exist_ok=True)
    for source,box,name in PARTS:
        crop(RAW/(source+'-light.png'),box,name)
    private=ROOT.parent/'target/doc-audit/settings-last/settings-light.png'
    if private.exists():
        for box,name in [((405,82,1256,188),'settings-layout'),
                         ((405,239,1256,429),'settings-connection'),
                         ((405,440,1256,713),'settings-shortcuts'),
                         ((405,724,1256,932),'settings-about')]:
            crop(private,box,name)
    print('Logical screenshot parts prepared; private profile path excluded')
