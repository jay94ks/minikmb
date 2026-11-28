"""Read-only mechanical extraction from the manufactured MiniKMB KiCad board."""
import sys
import hashlib
import json
from pathlib import Path
import pcbnew

ROOT = Path(__file__).resolve().parent
SOURCE = Path(sys.argv[1]).resolve() if len(sys.argv)>1 else ROOT.parent/'stm32h7'/'MiniKMB.kicad_pcb'
board = pcbnew.LoadBoard(str(SOURCE))
origin = (169.615, 79.01)
def xy(v):
    return [round(pcbnew.ToMM(v.x)-origin[0], 5), round(origin[1]+54-pcbnew.ToMM(v.y), 5)]
def bounds(rect):
    a,b = xy(rect.GetPosition()),xy(rect.GetEnd())
    return [min(a[0],b[0]),min(a[1],b[1]),max(a[0],b[0]),max(a[1],b[1])]
footprints = []
for fp in board.GetFootprints():
    pads = []
    for p in fp.Pads():
        pads.append({'number':p.GetNumber(), 'xy':xy(p.GetPosition()),
                     'bbox':bounds(p.GetBoundingBox()),
                     'drill':[pcbnew.ToMM(p.GetDrillSize().x),pcbnew.ToMM(p.GetDrillSize().y)],
                     'attribute':int(p.GetAttribute())})
    courtyards = [bounds(g.GetBoundingBox()) for g in fp.GraphicalItems()
                  if g.GetLayerName() in ('F.CrtYd','B.CrtYd')]
    footprints.append({'ref':fp.GetReference(),'value':fp.GetValue(),
        'footprint':str(fp.GetFPID().GetLibItemName()),'xy':xy(fp.GetPosition()),
        'angle':fp.GetOrientationDegrees(),'layer':fp.GetLayerName(),
        'bounds':bounds(fp.GetBoundingBox(False,False)), 'courtyards':courtyards,
        'models':[m.m_Filename for m in fp.Models()], 'pads':pads})
data = {'source':'../stm32h7/MiniKMB.kicad_pcb','sha256':hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
        'coordinate_system':'mm; x right, y up from lower-left PCB; PCB bottom z=0',
        'width':80.,'depth':54.,'thickness':pcbnew.ToMM(board.GetDesignSettings().GetBoardThickness()),
        'footprints':footprints}
(ROOT/'board-mechanical.json').write_text(json.dumps(data,ensure_ascii=False,indent=2),encoding='utf-8')
print('SHA',data['sha256'],'bottom components',[f['ref'] for f in footprints if f['layer']=='B.Cu'])
for f in footprints:
    if f['ref'].startswith(('J','H','SW')):
        print(f['ref'], f['bounds'], f['models'])
print('NPTH',[(f['ref'],p['xy'],p['drill']) for f in footprints for p in f['pads'] if p['attribute']==2])
