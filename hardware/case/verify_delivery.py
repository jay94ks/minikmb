"""Verify the final, GUI-saved native document and package only current parts."""
import sys,json,hashlib,zipfile
from pathlib import Path
import FreeCAD as App

root=Path(__file__).resolve().parent
out=root/'output'
r=json.loads((out/'validation.json').read_text())
preview=json.loads((out/'preview-status.json').read_text())
assert preview['ok'] and preview['document'].startswith('MiniKVM_Clear_Snap_Case'),preview
assert preview['native_sha256']==hashlib.sha256((out/'MiniKVM_Clear_Snap_Case.FCStd').read_bytes()).hexdigest()
d=App.openDocument(str(out/'MiniKVM_Clear_Snap_Case.FCStd'))
d.recompute()
base=next(o for o in d.Objects if o.Label.startswith('01 BASE SNAP'))
lid=next(o for o in d.Objects if o.Label.startswith('02 LID SNAP'))
for name,obj in [('base',base),('lid',lid)]:
    assert obj.Shape.isValid() and obj.Shape.isClosed() and len(obj.Shape.Solids)==1
    assert abs(obj.Shape.Volume/1000-r[name]['volume_cm3'])<1e-7,(name,obj.Shape.Volume)
assert lid.Placement.Base.Length<1e-7,lid.Placement.Base
assert base.Shape.common(lid.Shape).Volume<0.001
assert d.getObject('HookL') and d.getObject('HookR')
assert all('M2' not in o.Label and 'screw' not in o.Label.lower() for o in d.Objects)
assert abs(d.getObject('HookL').Shape.BoundBox.YMin-30)<0.001
assert abs(d.getObject('HookL').Shape.BoundBox.YMax-34)<0.001
r['native_reopen_recompute']='passed after final snap-case GUI save'
r['preview_status']=preview
r['part_files']={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in out.iterdir()
                 if p.suffix.lower() in ('.fcstd','.step','.stl')}
(out/'validation.json').write_text(json.dumps(r,indent=2),encoding='utf-8')
part_names=['01_Base.step','01_Base.stl','02_Lid.step','02_Lid.stl']
with zipfile.ZipFile(out/'JLC3DP_STL.zip','w',zipfile.ZIP_DEFLATED) as z:
    for name in ['01_Base.stl','02_Lid.stl']: z.write(out/name,name)
with zipfile.ZipFile(out/'JLC3DP_STL.zip') as z:
    assert z.namelist()==['01_Base.stl','02_Lid.stl']
    assert z.testzip() is None
manifest={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in out.iterdir()
          if p.suffix.lower() in ('.fcstd','.step','.stl','.zip')}
(out/'SHA256.json').write_text(json.dumps(manifest,indent=2))
App.closeDocument(d.Name)
print('PASS: snap-only native reopen/recompute, exported parts, GUI save, ZIP integrity and hashes')
