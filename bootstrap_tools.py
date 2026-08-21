"""Install the tested public 3DS Texture Forge source dependency into ./tools.
No game files, keys, or proprietary assets are downloaded.
"""
from pathlib import Path
from urllib.request import urlopen, Request
from zipfile import ZipFile
from io import BytesIO
import shutil
import sys

FORGE_COMMIT='f3fc44bd83f111264bb83e1ea0019b82f8da7931'
URL=f'https://github.com/ZoomiesZaggy/3DS-Texture-Forge/archive/{FORGE_COMMIT}.zip'
root=Path(__file__).resolve().parent
tools=root/'tools'; tools.mkdir(exist_ok=True)
dest=tools/'3DS-Texture-Forge'
marker=dest/'.eouhd_commit'
force='--force' in sys.argv

if not force and (dest/'main.py').is_file() and marker.is_file() and marker.read_text(errors='ignore').strip() == FORGE_COMMIT:
    print('3DS Texture Forge already at tested revision', FORGE_COMMIT[:12])
    raise SystemExit(0)

print('Downloading tested 3DS Texture Forge revision', FORGE_COMMIT[:12]+'…')
req=Request(URL,headers={'User-Agent':'Etrian-Odyssey-HD-Texture-Extractor/0.12'})
with urlopen(req,timeout=90) as r:
    data=r.read()
with ZipFile(BytesIO(data)) as z:
    top=z.namelist()[0].split('/')[0]
    tmp=tools/'_forge_tmp'
    if tmp.exists(): shutil.rmtree(tmp)
    z.extractall(tmp)
    if dest.exists(): shutil.rmtree(dest)
    shutil.move(str(tmp/top),str(dest)); shutil.rmtree(tmp,ignore_errors=True)
marker.write_text(FORGE_COMMIT+'\n',encoding='ascii')
print('Installed to',dest)
