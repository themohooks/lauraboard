"""Validate local Markdown navigation and PNG integrity without dependencies.

Run: python docs/tools/verify_docs.py
Optional: --report writes docs/audit/link-image-check.json.
External HTTP links are listed, not requested. This is not a Markdown renderer.
"""
from pathlib import Path
import argparse
import collections
import json
import re
import struct
import unicodedata
import urllib.parse
import zlib

ROOT = Path(__file__).resolve().parents[1]
LINK = re.compile(r'!?\[[^\]\n]*\]\(([^)\n]+)\)')

def plain(text):
    return re.sub(r'```[^\n]*\n.*?```', '', text, flags=re.S)

def anchors(text):
    counts = collections.Counter()
    out = set()
    for title in re.findall(r'^#{1,6}\s+(.+?)\s*#*$', plain(text), flags=re.M):
        title = re.sub(r'\[([^\]]+)\]\([^)]+\)', r'\1', title)
        title = title.replace('`', '').lower()
        slug = ''.join(c for c in title if c in '-_ ' or unicodedata.category(c)[0] in 'LN')
        slug = slug.replace(' ', '-')
        n=counts[slug]; counts[slug]+=1
        out.add(slug + (f'-{n}' if n else ''))
    return out

def png_info(path):
    data=path.read_bytes()
    if data[:8]!=b'\x89PNG\r\n\x1a\n':
        raise ValueError('invalid PNG signature')
    pos=8; width=height=0; end=False; image=[]
    while pos<len(data):
        length=struct.unpack('>I',data[pos:pos+4])[0]
        kind=data[pos+4:pos+8]; chunk=data[pos+8:pos+8+length]
        crc=struct.unpack('>I',data[pos+8+length:pos+12+length])[0]
        if zlib.crc32(kind+chunk)&0xffffffff!=crc:
            raise ValueError('PNG CRC mismatch')
        if kind==b'IHDR': width,height=struct.unpack('>II',chunk[:8])
        if kind==b'IDAT': image.append(chunk)
        pos+=12+length
        if kind==b'IEND': end=True; break
    if not end or not width or not height or not zlib.decompress(b''.join(image)):
        raise ValueError('incomplete PNG')
    return {'width':width,'height':height,'bytes':len(data)}

def verify():
    files=sorted(ROOT.rglob('*.md'))
    errors=[]; count=0; external=set(); linked=set(); pictures={}
    for p in files:
        text=p.read_text('utf8')
        fences=re.findall(r'^\s*(`{3,}|~{3,})',text,re.M)
        if len(fences)%2: errors.append(f'{p.relative_to(ROOT)}: unbalanced code fences')
        for match in LINK.finditer(plain(text)):
            target=match.group(1).strip().split(' "',1)[0].strip('<>')
            if target.startswith(('https://','http://','mailto:')):
                external.add(target);continue
            if target.startswith(('data:','codex:')):continue
            path,_,anchor=target.partition('#')
            dest=(p.parent/urllib.parse.unquote(path)).resolve() if path else p.resolve()
            count+=1
            if not dest.exists():
                errors.append(f'{p.relative_to(ROOT)}: missing {target}');continue
            linked.add(dest)
            if anchor and dest.suffix=='.md' and urllib.parse.unquote(anchor) not in anchors(dest.read_text('utf8')):
                errors.append(f'{p.relative_to(ROOT)}: missing anchor {target}')
        # Table rows must agree with the adjacent separator row.
        rows=[]
        for line in plain(text).splitlines()+['']:
            if line.startswith('|'):
                rows.append(line)
            else:
                if len(rows)>1 and re.fullmatch(r'[\s|:\-]+',rows[1]):
                    n=len(re.split(r'(?<!\\)\|',rows[0]))
                    if any(len(re.split(r'(?<!\\)\|',row))!=n for row in rows[1:]):
                        errors.append(f'{p.relative_to(ROOT)}: inconsistent table columns')
                rows=[]
        if re.search(r'C:[/\\]Users[/\\](?!operator\b)|D:[/\\]Maksim',text,re.I):
            errors.append(f'{p.relative_to(ROOT)}: personal workstation path')
    for p in sorted((ROOT/'screenshots').rglob('*.png')):
        try:pictures[p.relative_to(ROOT).as_posix()]=png_info(p)
        except Exception as e:errors.append(f'{p.relative_to(ROOT)}: {e}')
    orphan_pages=[p.relative_to(ROOT).as_posix() for p in files if p.resolve() not in linked and p.name!='README.md']
    orphan_images=[p for p in pictures if (ROOT/p).resolve() not in linked]
    if orphan_pages:errors.append('Unlinked Markdown pages: '+', '.join(orphan_pages))
    if orphan_images:errors.append('Unlinked screenshots: '+', '.join(orphan_images))
    return {'markdown_files':len(files),'local_links_checked':count,'png_files':len(pictures),
            'external_links_not_checked':sorted(external),'errors':errors,'images':pictures}

if __name__=='__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('--report',action='store_true'); args=parser.parse_args()
    result=verify()
    if args.report:
        folder=ROOT/'audit';folder.mkdir(exist_ok=True)
        (folder/'link-image-check.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
    print(json.dumps({k:v for k,v in result.items() if k!='images'},ensure_ascii=False,indent=2))
    raise SystemExit(1 if result['errors'] else 0)
