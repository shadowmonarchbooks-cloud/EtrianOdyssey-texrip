import json, tempfile, unittest, sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PIL import Image
from eouhd.workspace import ensure_workspace, rgba_hashes, import_runtime_dump

class ImportPackTests(unittest.TestCase):
    def test_pack_json_exact(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td)/'ws'; ensure_workspace(root)
            orig=root/'04_originals'/'ui'/'ui_TEST.png'; orig.parent.mkdir(parents=True,exist_ok=True)
            master=root/'05_hd_masters'/'ui'/'ui_TEST.png'; master.parent.mkdir(parents=True,exist_ok=True)
            im=Image.new('RGBA',(8,8),(10,20,30,255)); im.save(orig); im.save(master)
            dh,fh=rgba_hashes(orig)
            manifest={'title_id':'00040000000EC700','assets':[{'asset_id':'ui_TEST','category':'ui','width':8,'height':8,'format':0,'candidate_hash':'AAAAAAAAAAAAAAAA','verified_hashes':[],'original':str(orig.relative_to(root)).replace('\\','/'),'master':str(master.relative_to(root)).replace('\\','/'),'rgba_sha256':dh,'rgba_flip_sha256':fh}]}
            (root/'manifest.json').write_text(json.dumps(manifest))
            pack=Path(td)/'pack'; pack.mkdir(); im.save(pack/'pretty.png')
            (pack/'pack.json').write_text(json.dumps({'textures':{'1234567890ABCDEF':'pretty.png'}}))
            r=import_runtime_dump(root,pack)
            self.assertEqual(r['matched_exact'],1)
            m=json.loads((root/'manifest.json').read_text())
            self.assertIn('1234567890ABCDEF',m['assets'][0]['verified_hashes'])

if __name__=='__main__': unittest.main()
