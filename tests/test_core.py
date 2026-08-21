import json, struct, tempfile, unittest
from pathlib import Path
from PIL import Image
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))

from eouhd.hpx import parse_hpi, unpack_hpi_pair
from eouhd.workspace import parse_azahar_filename, category_for, detect_edited_masters, ensure_workspace, build_azahar_pack
from eouhd.eou_stex import (
    parse_eou_stex, FMT_RGBA4, FMT_RGBA5551, FMT_RGBA8,
    DT_UNSIGNED_SHORT_4444, DT_UNSIGNED_SHORT_5551, DT_UNSIGNED_BYTE, PF_RGBA,
)
from eouhd.strict_scan import _embedded_bch_offsets
import xxhash
from eouhd.cityhash64 import cityhash64_hex


def make_stex(data_type: int, pixel_format: int, payload_size: int, width: int = 8, height: int = 8):
    hdr = bytearray(0x80)
    hdr[:4] = b'STEX'
    struct.pack_into('<I', hdr, 0x0C, width)
    struct.pack_into('<I', hdr, 0x10, height)
    struct.pack_into('<I', hdr, 0x14, data_type)
    struct.pack_into('<I', hdr, 0x18, pixel_format)
    struct.pack_into('<I', hdr, 0x1C, payload_size)
    struct.pack_into('<I', hdr, 0x20, 0x80)
    hdr[0x28:0x30] = b'testtex\0'
    return bytes(hdr) + bytes((i & 0xFF for i in range(payload_size)))


class CoreTests(unittest.TestCase):
    def test_xxh64_vectors(self):
        self.assertEqual(xxhash.xxh64(b'').hexdigest(), 'ef46db3751d8e999')
        self.assertEqual(xxhash.xxh64(b'hello').hexdigest(), '26c7827d889f6da3')


    def test_cityhash64_azahar_vectors(self):
        self.assertEqual(cityhash64_hex(b''), '9AE16A3B2F90404F')
        self.assertEqual(cityhash64_hex(b'hello'), 'B48BE5A931380CE8')

    def test_filename(self):
        x=parse_azahar_filename('tex1_256x128_1234ABCD1234ABCD_13_mip0.png')
        self.assertEqual(x['width'],256); self.assertEqual(x['hash'],'1234ABCD1234ABCD')

    def test_category(self):
        self.assertEqual(category_for('/data/ui/menu/foo.stex'),'ui')
        self.assertEqual(category_for('/enemy/boss01.stex'),'monsters')

    def test_uncompressed_hpi(self):
        with tempfile.TemporaryDirectory() as td:
            td=Path(td); hpi=td/'x.hpi'; hpb=td/'x.hpb'; payload=b'STEXdemo'
            name=b'foo/test.stex\x00'
            header=b'HPIH'+struct.pack('<IIIHHHH',0,0,0,0,0,1,0)
            entry=struct.pack('<IIII',0,0,len(payload),0)
            hpi.write_bytes(header+entry+name); hpb.write_bytes(payload)
            es=parse_hpi(hpi); self.assertEqual(es[0].filename,'foo/test.stex')
            out=td/'out'; unpack_hpi_pair(hpi,out)
            self.assertEqual((out/'foo/test.stex').read_bytes(),payload)

    def test_eou_stex_uses_type_plus_pixel_format(self):
        # All three use PF_RGBA; only the data-type field distinguishes their
        # actual storage. This is the v0.1 corruption regression test.
        s4444 = parse_eou_stex(make_stex(DT_UNSIGNED_SHORT_4444, PF_RGBA, 128))
        s5551 = parse_eou_stex(make_stex(DT_UNSIGNED_SHORT_5551, PF_RGBA, 128))
        s8888 = parse_eou_stex(make_stex(DT_UNSIGNED_BYTE, PF_RGBA, 256))
        self.assertEqual(s4444.pica_format, FMT_RGBA4)
        self.assertEqual(s5551.pica_format, FMT_RGBA5551)
        self.assertEqual(s8888.pica_format, FMT_RGBA8)
        self.assertEqual(len(s4444.raw), 128)

    def test_embedded_bch_bam2_detection(self):
        wrapper = bytearray(b'BAM2' + b'\x00' * 0x3C)
        off = len(wrapper)
        bch = bytearray(0x80)
        bch[:4] = b'BCH\x00'
        # Header section pointers. data_addr must be a valid in-payload offset.
        struct.pack_into('<IIIIII', bch, 0x08, 0x20, 0x28, 0x30, 0x40, 0x00, 0x70)
        blob = bytes(wrapper + bch)
        self.assertEqual(_embedded_bch_offsets(blob), [off])


    def test_pack_uses_pack_json_friendly_filename(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td); ensure_workspace(root)
            orig=root/'04_originals/ui/a.png'; master=root/'05_hd_masters/ui/a.png'
            orig.parent.mkdir(parents=True,exist_ok=True); master.parent.mkdir(parents=True,exist_ok=True)
            Image.new('RGBA',(16,16),(1,2,3,255)).save(orig); Image.open(orig).save(master)
            manifest={'title_id':'00040000000EC700','assets':[{'asset_id':'ui_A','category':'ui','width':8,'height':8,'format':0,'candidate_hash':'B48BE5A931380CE8','verified_hashes':[],'mip':0,'original':'04_originals/ui/a.png','master':'05_hd_masters/ui/a.png'}]}
            (root/'manifest.json').write_text(json.dumps(manifest))
            out=build_azahar_pack(root, use_candidates=True)
            self.assertTrue((out/'ui'/'ui-a.png').is_file())
            pack=json.loads((out/'pack.json').read_text())
            self.assertEqual(pack['textures']['B48BE5A931380CE8'],'ui-a.png')
            pack=json.loads((out/'pack.json').read_text())
            self.assertTrue(pack['options']['use_new_hash'])

    def test_preserve_only_edited_masters(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td)
            (root/'04_originals/misc').mkdir(parents=True)
            (root/'05_hd_masters/misc').mkdir(parents=True)
            o1=root/'04_originals/misc/a.png'; m1=root/'05_hd_masters/misc/a.png'
            o2=root/'04_originals/misc/b.png'; m2=root/'05_hd_masters/misc/b.png'
            Image.new('RGBA',(8,8),(1,2,3,255)).save(o1); Image.open(o1).save(m1)
            Image.new('RGBA',(8,8),(4,5,6,255)).save(o2); Image.new('RGBA',(16,16),(4,5,6,255)).save(m2)
            manifest={'assets':[
                {'asset_id':'a','original':'04_originals/misc/a.png','master':'05_hd_masters/misc/a.png'},
                {'asset_id':'b','original':'04_originals/misc/b.png','master':'05_hd_masters/misc/b.png'}]}
            (root/'manifest.json').write_text(json.dumps(manifest))
            self.assertEqual(detect_edited_masters(root), {'b'})

if __name__=='__main__': unittest.main()
