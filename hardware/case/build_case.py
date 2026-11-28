"""Build editable FreeCAD CSG model and individually printable STEP/STL parts.

Run with the installed FreeCAD Python interpreter. No firmware or PCB edits.
The spreadsheet and standard Part primitives/booleans remain editable on reopen.
"""
import sys, json, math, hashlib
from pathlib import Path
import FreeCAD as App
import Part, MeshPart
print('FreeCAD modules loaded',flush=True)

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'output'
OUT.mkdir(exist_ok=True)
DOC = App.newDocument('MiniKVM_Clear_Snap_Case')
P = DOC.addObject('Spreadsheet::Sheet', 'Parameters')
P.Label = 'Parameters (mm) — edit values in column B'
settings = [
    ('BoardWidth',80.0),('BoardDepth',54.0),('BoardThickness',1.5584),
    ('Wall',1.6),('Floor',1.6),('Lid',1.6),('Height',19.2),
    ('BoardZ',5.5),('SideExtra',6.5),('EndExtra',2.8),
    ('CornerRadius',2.8),
    ('CaptureGap',0.6),('SideGap',0.5),('SupportOverlap',1.5),
]
for row,(name,value) in enumerate(settings,1):
    P.set(f'A{row}',name); P.set(f'B{row}',str(value)+' mm'); P.setAlias(f'B{row}',name)
P.set('A19','Units: mm. Source PCB 80 x 54. All dimensions nominal.')
P.set('A20','NOT FIT TESTED; check missing models, plugs, SD travel and temperature.')
P.setColumnWidth('A',540); P.setColumnWidth('B',130)
P.setStyle('A1:A16','bold')
history=DOC.addObject('App::DocumentObjectGroup','Construction')
history.Label='Editable primitives and boolean history'
refs=DOC.addObject('App::DocumentObjectGroup','References')
refs.Label='PCB references — do not upload to JLC3DP'
seq=0
def expr(obj,path,value):
    if isinstance(value,str): obj.setExpression(path,value)
    else: setattr(obj,path,value)
def box(label,x,y,z,dx,dy,dz):
    global seq
    seq+=1
    o=DOC.addObject('Part::Box',f'Box{seq:03d}'); o.Label=label
    for attr,val in [('Length',dx),('Width',dy),('Height',dz)]: expr(o,attr,val)
    for axis,val in zip('xyz',[x,y,z]):
        if isinstance(val,str): o.setExpression('Placement.Base.'+axis,val)
        else:
            v=o.Placement.Base; setattr(v,axis,val); o.Placement.Base=v
    history.addObject(o); return o
def cyl(label,x,y,z,r,h):
    global seq
    seq+=1
    o=DOC.addObject('Part::Cylinder',f'Cylinder{seq:03d}'); o.Label=label
    expr(o,'Radius',r); expr(o,'Height',h)
    for axis,val in zip('xyz',[x,y,z]):
        if isinstance(val,str): o.setExpression('Placement.Base.'+axis,val)
        else:
            v=o.Placement.Base; setattr(v,axis,val); o.Placement.Base=v
    history.addObject(o); return o
def fuse(label,items):
    o=DOC.addObject('Part::MultiFuse',f'Fuse{len(DOC.Objects):03d}'); o.Label=label
    o.Shapes=items; o.Refine=True; history.addObject(o); return o
def cut(label,base,tool):
    o=DOC.addObject('Part::Cut',f'Cut{len(DOC.Objects):03d}'); o.Label=label
    o.Base=base; o.Tool=tool; o.Refine=True; history.addObject(o); return o
def rr(label,x,y,z,w,d,h,r):
    # Symbolic expressions make the full rounded rectangular shell editable.
    a=box(label+' central X',f'({x})+({r})',y,z,f'({w})-2*({r})',d,h)
    b=box(label+' central Y',x,f'({y})+({r})',z,w,f'({d})-2*({r})',h)
    circles=[]
    for xx in [f'({x})+({r})',f'({x})+({w})-({r})']:
        for yy in [f'({y})+({r})',f'({y})+({d})-({r})']:
            circles.append(cyl(label+' corner',xx,yy,z,r,h))
    return fuse(label,[a,b]+circles)

W='Parameters.BoardWidth+2*Parameters.SideExtra'
D='Parameters.BoardDepth+2*Parameters.EndExtra'
BH='Parameters.Height-Parameters.Lid'
outer=rr('Base outer rounded envelope','-Parameters.SideExtra','-Parameters.EndExtra',0,W,D,BH,'Parameters.CornerRadius')
inner=rr('Open interior', '-Parameters.SideExtra+Parameters.Wall','-Parameters.EndExtra+Parameters.Wall',
         'Parameters.Floor',f'({W})-2*Parameters.Wall',f'({D})-2*Parameters.Wall',
         f'({BH})-Parameters.Floor+1 mm','Parameters.CornerRadius-Parameters.Wall')
tray=cut('Thin open tray',outer,inner)
supports=[]; stops=[]
for side in ['L','R']:
    for y in [4,46]:
        x='-Parameters.SideExtra+Parameters.Wall-0.1 mm' if side=='L' else 'Parameters.BoardWidth-Parameters.SupportOverlap'
        length='Parameters.SideExtra-Parameters.Wall+Parameters.SupportOverlap+0.1 mm'
        supports.append(box(f'{side} bare-edge seat y{y}',x,y,'Parameters.Floor',length,4,'Parameters.BoardZ-Parameters.Floor'))
        # Vertical edge fences stop translation without touching PCB faces.
        sx='-Parameters.SideExtra+Parameters.Wall-0.1 mm' if side=='L' else 'Parameters.BoardWidth+Parameters.SideGap'
        sl='Parameters.SideExtra-Parameters.Wall-Parameters.SideGap+0.1 mm'
        stops.append(box(f'{side} lateral fence y{y}',sx,y,'Parameters.BoardZ',sl,4,'Parameters.BoardThickness+Parameters.CaptureGap'))
for x in [0,78]:
    for end in ['S','N']:
        yy='-Parameters.EndExtra+Parameters.Wall-0.1 mm' if end=='S' else 'Parameters.BoardDepth+Parameters.SideGap'
        dd='Parameters.EndExtra-Parameters.Wall-Parameters.SideGap+0.1 mm'
        stops.append(box(f'{end} longitudinal stop x{x}',x,yy,'Parameters.Floor',2,dd,
                         'Parameters.BoardZ+Parameters.BoardThickness-Parameters.Floor'))
base_stock=fuse('Tray and PCB edge seats',[tray]+supports+stops)

# Openings include actual connector body plus access allowance. Cable overmolds
# and microSD push-push travel must still be checked against the real hardware.
ports=[
    ('J1 HDMI channel 0',3.625,20.6,5.8,9.5,'N'),
    ('J3 USB-C target 0',24.625,12.2,5.8,8.0,'N'),
    ('J2 HDMI channel 1',42.905,20.6,5.8,9.5,'N'),
    ('J4 USB-C target 1',63.905,12.2,5.8,8.0,'N'),
    ('J7 microSD 0 finger access',3.9,15.5,5.0,5.3,'S'),
    ('J8 microSD 1 finger access',18.825,15.5,5.0,5.3,'S'),
    ('J9 USB-C management',62.9,14.2,5.0,8.0,'S'),
]
openings=[]
for label,x,width,z,height,end in ports:
    yy='Parameters.BoardDepth-0.1 mm' if end=='N' else '-Parameters.EndExtra-1 mm'
    openings.append(box(label+' opening',x,yy,z,width,'Parameters.EndExtra+1.1 mm',height))
# The two SD access cuts deliberately merge: no fragile 0.575 mm separator.
openings.append(box('Merged SD finger-access bridge',18.8,'-Parameters.EndExtra-1 mm',5.0,0.7,'Parameters.EndExtra+1.1 mm',5.3))
# Two side windows receive the hooks. The 3.2 mm rim above each window
# provides a horizontal retaining ledge and permits finger access for release.
receivers=[box('Left release window', '-Parameters.SideExtra-1 mm',27,9,
               'Parameters.Wall+2 mm',10,f'({BH})-12.2 mm'),
           box('Right release window','Parameters.BoardWidth+Parameters.SideExtra-Parameters.Wall-1 mm',27,9,
               'Parameters.Wall+2 mm',10,f'({BH})-12.2 mm')]
base=cut('Base — 1 printable solid',base_stock,fuse('Base opening tools',openings+receivers))
base.Label='01 BASE SNAP — clear 8001, qty 1'

lid_plate=rr('Lid thin plate','-Parameters.SideExtra','-Parameters.EndExtra',BH,W,D,'Parameters.Lid','Parameters.CornerRadius')
keepers=[]
for x in ['-0.1 mm','Parameters.BoardWidth-1.5 mm']:
    for y in [4,46]:
        keepers.append(box('Lid edge keeper — 0.6 mm Z clearance',x,y,
                           'Parameters.BoardZ+Parameters.BoardThickness+Parameters.CaptureGap',
                           1.6,4,
                           f'({BH})-Parameters.BoardZ-Parameters.BoardThickness-Parameters.CaptureGap'))
# End tabs guide a vertical push-to-close lid; there is no sliding sweep through
# the PCB components. Nominal clearance to the inner end walls is 0.5 mm.
for x in [10,68]:
    for y in ['-Parameters.EndExtra+Parameters.Wall+0.5 mm',
              'Parameters.BoardDepth+Parameters.EndExtra-Parameters.Wall-2.1 mm']:
        keepers.append(box('Lid alignment tongue',x,y,f'({BH})-2 mm',6,1.6,2.1))

# Each latch is a horizontal 26 mm long cantilever inside the side channel.
# It flexes inward in X while the sloping nose crosses the side rim. The hook
# then enters the release window and catches below its top edge.
import Sketcher
print('Building native snap latch sketches',flush=True)
latches=[]
for side in ['L','R']:
    xx='-Parameters.SideExtra+Parameters.Wall+0.7 mm' if side=='L' else 'Parameters.BoardWidth+Parameters.SideExtra-Parameters.Wall-2.3 mm'
    latches.append(box(f'{side} latch root',xx,5,f'({BH})-6.6 mm',1.6,3,6.7))
    latches.append(box(f'{side} long flexure 26 x 1.6 x 3 mm',xx,8,f'({BH})-6.6 mm',1.6,26,3))
    # Add an R1 concave root blend instead of leaving a sharp flexure shoulder.
    blend_box=box(f'{side} root blend stock',xx,8,f'({BH})-3.6 mm',1.6,1,1)
    blend_tool=cyl(f'{side} root blend R1 cutter',f'({xx})-0.1 mm',9,f'({BH})-2.6 mm',1,1.8)
    blend_tool.Placement.Rotation=App.Rotation(App.Vector(0,1,0),90)
    latches.append(cut(f'{side} R1 latch root relief',blend_box,blend_tool))
    sk=DOC.addObject('Sketcher::SketchObject',f'HookProfile{side}')
    sk.Label=f'{side} latch ramp profile — editable sketch'
    sk.Placement.Rotation=App.Rotation(App.Vector(1,0,0),90)
    sk.Placement.Base=App.Vector(0,30,0)
    sk.setExpression('Placement.Base.x','-Parameters.SideExtra+Parameters.Wall+0.7 mm' if side=='L'
                     else 'Parameters.BoardWidth+Parameters.SideExtra-Parameters.Wall-0.7 mm')
    sk.setExpression('Placement.Base.z',f'({BH})-6.7 mm')
    sign=-1 if side=='L' else 1
    pts=[(0,0),(sign*1.5,1.5),(sign*1.5,3.1),(-sign*.15,3.1),(-sign*.15,0)]
    for a,b in zip(pts,pts[1:]+pts[:1]):
        idx=sk.addGeometry(Part.LineSegment(App.Vector(*a,0),App.Vector(*b,0)),False)
        sk.addConstraint(Sketcher.Constraint('Block',idx))
    history.addObject(sk)
    hook=DOC.addObject('Part::Extrusion',f'Hook{side}'); hook.Label=f'{side} ramped snap hook'
    hook.Base=sk; hook.DirMode='Custom'; hook.Dir=App.Vector(0,4,0); hook.Solid=True
    history.addObject(hook); latches.append(hook)
lid_stock=fuse('Lid, PCB retainers and integral release latches', [lid_plate]+keepers+latches)
vents=[]
for start in [5.2,44.48]:
    for n in range(7):
        vents.append(box('Top cooling slot',start+3.6*n,27,f'({BH})-0.1 mm',1.8,17,'Parameters.Lid+0.2 mm'))
# SWD plugs fit through dedicated top service windows (no extra printed caps).
service=[box('J5 SWD service opening',-.15,12.2,f'({BH})-0.1 mm',4.6,17.4,'Parameters.Lid+0.2 mm'),
         box('J6 SWD service opening',75.95,7.5,f'({BH})-0.1 mm',4.6,17.4,'Parameters.Lid+0.2 mm')]
lid=cut('Lid — 1 printable solid',lid_stock,fuse('Lid opening tools',vents+service))
lid.Label='02 LID SNAP — clear 8001, qty 1'

board_shape=Part.read(str(ROOT/'board-reference.step'))
board=DOC.addObject('Part::Feature','BoardReference'); board.Label='PCB STEP reference (8 missing models; see export log)'
board.Shape=board_shape; board.setExpression('Placement.Base.z','Parameters.BoardZ'); refs.addObject(board)
# Conservatively cover all missing 3D models with courtyard-based boxes.
data=json.loads((ROOT/'board-mechanical.json').read_text(encoding='utf-8'))
missing=['U4','U13','J4','U3','J3','U15','Y9','Y6']
proxy_shapes=[]
for f in data['footprints']:
    if f['ref'] not in missing: continue
    x0,y0,x1,y1=f['bounds']; bottom=f['layer']=='B.Cu'
    hh=3.5 if f['ref'].startswith('J') else 1.6
    zz=-hh if bottom else data['thickness']
    s=Part.makeBox(x1-x0,y1-y0,hh,App.Vector(x0,y0,zz))
    # Courtyard is conservative in XY; connector cut must include forward edge.
    proxy_shapes.append(s)
proxy=DOC.addObject('Part::Feature','MissingModels'); proxy.Label='AMBER — conservative envelopes of 8 missing models'
proxy.Shape=Part.makeCompound(proxy_shapes); proxy.setExpression('Placement.Base.z','Parameters.BoardZ'); refs.addObject(proxy)
DOC.recompute()
print('Geometry recomputed; validating',flush=True)

def metrics(o):
    s=o.Shape
    return {'valid':s.isValid(),'solids':len(s.Solids),'closed':s.isClosed(),
            'volume_cm3':s.Volume/1000,'bbox_mm':[s.BoundBox.XLength,s.BoundBox.YLength,s.BoundBox.ZLength]}
report={'status':'DESIGN PROTOTYPE — NOT FIT/THERMAL TESTED','source_sha256':data['sha256'],
        'base':metrics(base),'lid':metrics(lid),'missing_models':missing}
report['closure']='two integral side-release cantilever snap hooks; no fasteners or adhesive'
report['latch_nominal']={'beam_length_mm':26,'effective_length_to_hook_center_mm':24,
    'beam_flex_thickness_mm':1.6,'beam_depth_mm':3,'hook_ledge_overlap_mm':0.8,
    'root_blend_radius_mm':1.0,
    'vertical_play_mm':0.4,'assembly_flex_nominal_mm':1.1,'release_travel_checked_mm':1.4,
    'surface_strain_estimate_percent':100*1.5*1.6*1.4/(24*24),
    'strain_note':'small-deflection cantilever estimate only; not FEA or a fatigue/strength acceptance'}
for o in [base,lid]:
    assert o.Shape.isValid() and len(o.Shape.Solids)==1 and o.Shape.isClosed(),(o.Name,metrics(o))
for label,a,b in [('base_lid',base.Shape,lid.Shape),('base_PCB_STEP',base.Shape,board.Shape),
                  ('lid_PCB_STEP',lid.Shape,board.Shape),('base_missing_envelopes',base.Shape,proxy.Shape),
                  ('lid_missing_envelopes',lid.Shape,proxy.Shape)]:
    v=a.common(b).Volume
    report[label+'_intersection_mm3']=v
    assert v<0.001,(label,v)
flex_envelopes=[Part.makeBox(3.0,26,3,App.Vector(-4.2,8,11)),
                Part.makeBox(3.0,26,3,App.Vector(81.2,8,11)),
                Part.makeBox(3.05,4,3.1,App.Vector(-5.7,30,10.9)),
                Part.makeBox(3.05,4,3.1,App.Vector(82.65,30,10.9))]
flex_space=Part.makeCompound(flex_envelopes)
for label,target in [('base',base.Shape),('PCB_STEP',board.Shape),('missing_envelopes',proxy.Shape)]:
    v=flex_space.common(target).Volume
    report['release_space_'+label+'_intersection_mm3']=v
    assert v<0.001,(label,v)
report['total_volume_cm3']=report['base']['volume_cm3']+report['lid']['volume_cm3']
# Compare against 2.4 mm sheets/walls, same outside dimensions and features.
for name in ['Wall','Floor','Lid']:
    row=next(i for i,s in enumerate(settings,1) if s[0]==name); P.set(f'B{row}','2.4 mm')
DOC.recompute()
report['reference_2_4mm_volume_cm3']=(base.Shape.Volume+lid.Shape.Volume)/1000
report['resin_volume_reduction_percent']=100*(1-report['total_volume_cm3']/report['reference_2_4mm_volume_cm3'])
for name in ['Wall','Floor','Lid']:
    row=next(i for i,s in enumerate(settings,1) if s[0]==name); P.set(f'B{row}','1.6 mm')
DOC.recompute()

# These seat footprints contact only bare PCB edges: compare with *every*
# footprint's XY bounds, including solder pads/graphics, not just present STEP.
contacts=[[0,4,1.5,8],[0,46,1.5,50],[78.5,4,80,8],[78.5,46,80,50]]
conflicts=[]
for c in contacts:
    for f in data['footprints']:
        b=f['bounds']
        if min(c[2],b[2])-max(c[0],b[0])>0 and min(c[3],b[3])-max(c[1],b[1])>0:
            conflicts.append((c,f['ref']))
report['seat_xy_footprint_conflicts']=conflicts
assert not conflicts,conflicts

for obj in DOC.Objects:
    if hasattr(obj,'Visibility'): obj.Visibility=False
for obj in [base,lid,board,proxy]: obj.Visibility=True
# View settings are applied by the optional GUI macro; geometry generation also
# works with FreeCAD's bundled python.exe without any UI session.
DOC.recompute()
DOC.saveAs(str(OUT/'MiniKVM_Clear_Snap_Case.FCStd'))
for name,obj in [('01_Base',base),('02_Lid',lid)]:
    s=obj.Shape.copy()
    if name=='02_Lid':
        # Flip outside face onto XY: retained fingers extend up for review.
        s.rotate(App.Vector(0,0,0),App.Vector(1,0,0),180)
    s.translate(App.Vector(-s.BoundBox.XMin,-s.BoundBox.YMin,-s.BoundBox.ZMin))
    s.exportStep(str(OUT/(name+'.step')))
    mesh=MeshPart.meshFromShape(Shape=s,LinearDeflection=0.04,AngularDeflection=0.15,Relative=False)
    mesh.write(str(OUT/(name+'.stl')))
    report[name+'_mesh']={'closed':mesh.isSolid(),'facets':mesh.CountFacets,
                         'volume_cm3':mesh.Volume/1000,'non_manifolds':mesh.hasNonManifolds()}
    assert mesh.isSolid() and not mesh.hasNonManifolds()
    # Re-read the exported STEP to catch corrupt or empty manufacturing exports.
    exported=Part.read(str(OUT/(name+'.step')))
    assert exported.isValid() and len(exported.Solids)==1
    assert abs(exported.Volume-s.Volume)<0.001
report['part_files']={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in OUT.iterdir() if p.suffix.lower() in ['.stl','.step','.fcstd']}
(OUT/'validation.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
print(json.dumps(report,indent=2))
App.closeDocument(DOC.Name)
