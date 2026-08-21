import json
import tempfile
from pathlib import Path

from PIL import Image

from eouhd.workspace import (
    MASTER_PACK_DIR, assign_friendly_pack_names, build_azahar_pack,
    collect_protected_master_sources, ensure_workspace, rgba_hashes,
    save_manifest, sync_azahar_master_pack,
)

TITLE = '00040000000EC700'


def _asset(root: Path, *, aid: str, hash_: str, texture_name: str, source: str,
           category: str = 'monsters', bindings=None, color=(10,20,30,255)) -> dict:
    dirs = ensure_workspace(root)
    orig = dirs['originals']/category/f'{aid}.png'
    master = dirs['masters']/category/f'{aid}.png'
    orig.parent.mkdir(parents=True, exist_ok=True); master.parent.mkdir(parents=True, exist_ok=True)
    Image.new('RGBA',(8,8),color).save(orig); Image.open(orig).save(master)
    dh,fh = rgba_hashes(orig)
    return {
        'asset_id': aid, 'title_id': TITLE, 'category': category,
        'width':8,'height':8,'format':0,'candidate_hash':hash_,
        'verified_hashes':[],'mip':0,'original':str(orig.relative_to(root)).replace('\\','/'),
        'master':str(master.relative_to(root)).replace('\\','/'),
        'rgba_sha256':dh,'rgba_flip_sha256':fh,
        'texture_name':texture_name,'source':source,'material_bindings':bindings or [],
    }


def test_readable_names_and_pack_json_mapping():
    with tempfile.TemporaryDirectory() as td:
        root=Path(td)
        a=_asset(root,aid='mon_MONKEY',hash_='1111111111111111',texture_name='monkey',source='ENEMY/MONKEY.STEX')
        sync_azahar_master_pack(root,[a],{})
        save_manifest(root,TITLE,[a],version='0.12.0')
        out=build_azahar_pack(root)
        assert (out/'monsters'/'monkey.png').is_file()
        pack=json.loads((out/'pack.json').read_text())
        assert pack['textures']['1111111111111111']=='monkey.png'


def test_auxiliary_alpha_channel_gets_alpha_name():
    with tempfile.TemporaryDirectory() as td:
        root=Path(td)
        binding={'model_name':'monkey','slot':1,'alpha_uses':[{'operand':'Red'}]}
        a=_asset(root,aid='mon_MASK',hash_='2222222222222222',texture_name='monkey_mask',source='ENEMY/MONKEY.BAM',bindings=[binding])
        assign_friendly_pack_names([a])
        assert a['pack_filename']=='monkey-alpha.png'


def test_embedded_alpha_does_not_create_separate_alpha_name():
    with tempfile.TemporaryDirectory() as td:
        root=Path(td)
        binding={'model_name':'monkey','slot':0,'alpha_uses':[{'operand':'Alpha'}]}
        a=_asset(root,aid='mon_MAIN',hash_='3333333333333333',texture_name='monkey',source='ENEMY/MONKEY.BAM',bindings=[binding])
        assign_friendly_pack_names([a])
        assert a['pack_filename']=='monkey.png'


def test_multiple_hashes_share_one_physical_png():
    with tempfile.TemporaryDirectory() as td:
        root=Path(td)
        a=_asset(root,aid='mon_MONKEY',hash_='4444444444444444',texture_name='monkey',source='ENEMY/MONKEY.STEX')
        a['verified_hashes']=['AAAAAAAAAAAAAAAA','BBBBBBBBBBBBBBBB']
        sync_azahar_master_pack(root,[a],{},use_candidates=True)
        save_manifest(root,TITLE,[a],version='0.12.0')
        out=build_azahar_pack(root,use_candidates=True)
        pngs=list(out.rglob('*.png'))
        assert len(pngs)==1 and pngs[0].name=='monkey.png'
        pack=json.loads((out/'pack.json').read_text())
        assert pack['textures']['AAAAAAAAAAAAAAAA']=='monkey.png'
        assert pack['textures']['BBBBBBBBBBBBBBBB']=='monkey.png'


def test_global_basename_collision_gets_stable_hash_suffix():
    with tempfile.TemporaryDirectory() as td:
        root=Path(td)
        a=_asset(root,aid='A',hash_='ABCDEF0011111111',texture_name='shared',source='A.STEX')
        b=_asset(root,aid='B',hash_='1234567822222222',texture_name='shared',source='B.STEX',category='ui')
        assign_friendly_pack_names([a,b])
        names={a['pack_filename'],b['pack_filename']}
        assert 'shared.png' in names
        assert any(n.startswith('shared-') and n.endswith('.png') for n in names)


def test_migrates_edited_011_canonical_master_to_friendly_name():
    with tempfile.TemporaryDirectory() as td:
        root=Path(td)
        a=_asset(root,aid='mon_MONKEY',hash_='5555555555555555',texture_name='monkey',source='MONKEY.STEX')
        # Simulate a 0.11 manifest and edited canonical master.
        old=root/MASTER_PACK_DIR/'load'/'textures'/TITLE/'monsters'/'tex1_8x8_5555555555555555_0_mip0.png'
        old.parent.mkdir(parents=True,exist_ok=True)
        Image.new('RGBA',(32,32),(99,88,77,255)).save(old)
        old_asset=dict(a)
        old_asset['master']=str(old.relative_to(root)).replace('\\','/')
        old_asset['master_files']=[old_asset['master']]
        save_manifest(root,TITLE,[old_asset],version='0.11.0')
        protected=collect_protected_master_sources(root)
        fresh=_asset(root,aid='mon_MONKEY',hash_='5555555555555555',texture_name='monkey',source='MONKEY.STEX',color=(1,2,3,255))
        sync_azahar_master_pack(root,[fresh],protected)
        new=root/MASTER_PACK_DIR/'load'/'textures'/TITLE/'monsters'/'monkey.png'
        with Image.open(new) as im:
            assert im.size==(32,32)
            assert im.convert('RGBA').getpixel((0,0))==(99,88,77,255)
