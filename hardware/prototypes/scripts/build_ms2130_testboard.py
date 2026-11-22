"""Reproducible KiCad 10 capture test-board design. Run with KiCad's Python.

This is an engineering prototype, NOT a fabrication release. See DESIGN_STATUS.txt.
"""
from __future__ import annotations

import csv
import json
import math
import re
import shutil
import sys
import uuid
from collections import defaultdict
from pathlib import Path

import pcbnew as pcb

sys.dont_write_bytecode = True
import generate_kicad_libraries as gen

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "boards" / "ms2130-test"
LIB = OUT / "TestBoard.pretty"
KICAD = Path("C:/Program Files/KiCad/10.0/share/kicad/footprints")
NAME = "MS2130_Test"
NS = uuid.UUID("bde08669-3b61-477d-bbb5-ac13ce1ddf61")
ROOT_ID = str(uuid.uuid5(NS, NAME))


def uid(key):
    return str(uuid.uuid5(NS, str(key)))


def q(s):
    return '"' + str(s).replace("\\", "\\\\").replace('"', '\\"') + '"'


def vec(x, y):
    return pcb.VECTOR2I(pcb.FromMM(x), pcb.FromMM(y))


def mm(v):
    return pcb.ToMM(v)


def generic(name, ref, pins, fp, description=""):
    # Pins: number, name, electrical type, x, y, angle (library coordinates).
    body = '      (rectangle (start -7.62 7.62) (end 7.62 -7.62) (stroke (width 0.254) (type default)) (fill (type background)))'
    if name in ("FLAG", "TESTPOINT"):
        body = '      (circle (center 0 0) (radius 1.27) (stroke (width 0.254) (type default)) (fill (type none)))'
        body += '\n      (polyline (pts (xy -7.62 0) (xy -1.27 0)) (stroke (width 0.254) (type default)) (fill (type none)))'
        ref_y, value_y = 5.08, 2.54
    elif name in ("R", "C", "FB", "TVS"):
        if name == "C":
            body = '\n'.join(f'      (polyline (pts (xy {x} -2.54) (xy {x} 2.54)) (stroke (width 0.254) (type default)) (fill (type none)))' for x in (-0.635, 0.635))
            body += '\n' + '\n'.join(f'      (polyline (pts (xy {x} 0) (xy {xe} 0)) (stroke (width 0.254) (type default)) (fill (type none)))' for x, xe in ((-2.54, -0.635), (0.635, 2.54)))
        else:
            body = '      (rectangle (start -2.54 1.27) (end 2.54 -1.27) (stroke (width 0.254) (type default)) (fill (type none)))'
        ref_y, value_y = 5.08, 2.54
    else:
        ref_y, value_y = 12.7, 10.16
    props = [gen.prop("Reference", ref, 0, ref_y), gen.prop("Value", name, 0, value_y), gen.prop("Footprint", "TestBoard:" + fp, 0, 0, True), gen.prop("Datasheet", "", 0, 0, True), gen.prop("Description", description, 0, 0, True)]
    pinsex = [gen.pin_sexpr(gen.p(n, nm, typ), x, y, ang) for n, nm, typ, x, y, ang in pins]
    return '\n'.join([f'  (symbol "{name}" (pin_names (offset 0.5)) (in_bom yes) (on_board yes)', *props, f'    (symbol "{name}_0_1"', body, '    )', f'    (symbol "{name}_1_1"', *pinsex, '    )', '  )'])


def pinrow(items):
    # Balanced two-column functional symbol, 2.54 mm grid.
    left, right = items[:math.ceil(len(items)/2)], items[math.ceil(len(items)/2):]
    assert len(left) <= 6
    return [(n, nm, typ, -10.16, 5.08-i*2.54, 0) for i, (n, nm, typ) in enumerate(left)] + [(n, nm, typ, 10.16, 5.08-i*2.54, 180) for i, (n, nm, typ) in enumerate(right)]


DEFS = {}
PARTS = []


def copy_fp(lib, name):
    src = KICAD / (lib + ".pretty") / (name + ".kicad_mod")
    assert src.is_file(), src
    shutil.copyfile(src, LIB / src.name)
    return name


def add(ref, symbol, value, nets, at, angle=0, side="F", mpn="", note="", sch=None):
    PARTS.append(dict(ref=ref, symbol=symbol, value=value, nets={str(k):v for k,v in nets.items()}, at=at, angle=angle, side=side, mpn=mpn or value, note=note, sch=sch, uuid=uid(ref)))


def setup_libs():
    for d in (OUT, LIB, OUT / "review"):
        d.mkdir(parents=True, exist_ok=True)
    for s in gen.SYMBOLS:
        if s["name"] not in ("MS2130", gen.HDMI_NAME):
            continue
        text = gen.make_symbol(s).replace('"Future_ICS:', '"TestBoard:')
        DEFS[s["name"]] = text
        shutil.copyfile(ROOT / "footprints" / "Future_ICS.pretty" / (s["footprint"] + ".kicad_mod"), LIB / (s["footprint"] + ".kicad_mod"))
        if s['name']==gen.HDMI_NAME:
            dest=LIB/(s['footprint']+'.kicad_mod')
            dest.write_text(dest.read_text(encoding='utf-8').replace('(layer "F.SilkS")','(layer "F.Fab")'),encoding='utf-8')
    small = [(1, "1", "passive", -5.08, 0, 0), (2, "2", "passive", 5.08, 0, 180)]
    for name, ref, lib, fp in (("R", "R", "Resistor_SMD", "R_0603_1608Metric"), ("C", "C", "Capacitor_SMD", "C_0603_1608Metric"), ("FB", "FB", "Resistor_SMD", "R_0603_1608Metric"), ("TVS", "D", "Diode_SMD", "D_SOD-523")):
        DEFS[name] = generic(name, ref, small, copy_fp(lib, fp))
    xc = [(1,"VIN","power_in",-10.16,2.54,0),(2,"VSS","power_in",0,-10.16,90),(3,"CE","input",-10.16,-2.54,0),(4,"NC","no_connect",10.16,-2.54,180),(5,"VOUT","power_out",10.16,2.54,180)]
    DEFS["XC6220"] = generic("XC6220", "U", xc, copy_fp("Package_TO_SOT_SMD", "SOT-23-5"), "XC6220B MR SOT-25: 1 VIN, 2 VSS, 3 CE, 4 NC, 5 OUT")
    flash = [(1,"CS#","input"),(2,"DO","output"),(3,"WP#","input"),(4,"GND","power_in"),(5,"DI","input"),(6,"CLK","input"),(7,"HOLD#","input"),(8,"VCC","power_in")]
    DEFS["SPI_64KB"] = generic("SPI_64KB", "U", pinrow(flash), copy_fp("Package_SO", "SOIC-8_3.9x4.9mm_P1.27mm"), "512-kbit SPI flash; firmware contents required")
    usbpins = [(1,"VBUS","passive"),(2,"D-","passive"),(3,"D+","passive"),(4,"GND","passive"),(5,"HOST_RX-","passive"),(6,"HOST_RX+","passive"),(7,"DRAIN","passive"),(8,"HOST_TX-","passive"),(9,"HOST_TX+","passive"),("SH","SHELL","passive")]
    DEFS["USB3_A_PLUG"] = generic("USB3_A_PLUG", "J", pinrow(usbpins), copy_fp("Connector_USB", "USB3_A_Plug_Wuerth_692112030100_Horizontal"), "Device plug: contacts 5/6 carry MS2130 TX; 8/9 carry MS2130 RX")
    xtal = [(1,"XT1","passive",-10.16,2.54,0),(2,"CASE","passive",-10.16,-2.54,0),(3,"XT2","passive",10.16,2.54,180),(4,"CASE","passive",10.16,-2.54,180)]
    DEFS["XTAL"] = generic("XTAL", "Y", xtal, copy_fp("Crystal", "Crystal_SMD_3225-4Pin_3.2x2.5mm"))
    for n in (2,3,6):
        pins = [(i,str(i),"passive",-10.16,5.08-(i-1)*2.54,0) for i in range(1,n+1)]
        DEFS[f"HEADER{n}"] = generic(f"HEADER{n}", "J", pins, copy_fp("Connector_PinHeader_2.54mm",f"PinHeader_1x{n:02}_P2.54mm_Vertical"))
    DEFS["TESTPOINT"] = generic("TESTPOINT", "TP", [(1,"TP","passive",-10.16,0,0)], copy_fp("TestPoint", "TestPoint_Pad_D1.5mm"))
    DEFS["TESTPOINT"] = DEFS["TESTPOINT"].replace('(in_bom yes)', '(in_bom no)')
    # Explicit ERC drive source for externally supplied rails and bead outputs.
    DEFS["FLAG"] = generic("FLAG", "#FLG", [(1,"POWER","power_out",-10.16,0,0)], "", "ERC power flag; not a PCB component")
    DEFS["FLAG"] = DEFS["FLAG"].replace('(in_bom yes) (on_board yes)', '(in_bom no) (on_board no)')
    DEFS["FLAG"] = DEFS["FLAG"].replace('"TestBoard:"', '""')
    (OUT / "TestBoard.kicad_sym").write_text('(kicad_symbol_lib (version 20231120) (generator kicad_symbol_editor)\n'+'\n'.join(DEFS.values())+'\n)\n', encoding="utf-8")
    (OUT / "sym-lib-table").write_text('(sym_lib_table (lib (name "TestBoard")(type "KiCad")(uri "${KIPRJMOD}/TestBoard.kicad_sym")(options "")(descr "Self-contained prototype library")))\n',encoding="utf-8")
    (OUT / "fp-lib-table").write_text('(fp_lib_table (lib (name "TestBoard")(type "KiCad")(uri "${KIPRJMOD}/TestBoard.pretty")(options "")(descr "Copied KiCad footprints plus custom MS2130/HDMI")))\n',encoding="utf-8")


def parts():
    ms = {}
    for p in gen.MS2130:
        n, name = p['number'], p['name']
        if 'VSS' in name or name == 'EP':
            ms[n] = 'GND'
        elif name.startswith('AVDD33_USB'):
            ms[n] = 'AVDD33_USB'
        elif name.startswith('AVDD11_USB'):
            ms[n] = 'AVDD11_USB'
        elif name.startswith(('DVDD33','DVDD11','AVDD25','AVDD33','AVDD11')):
            ms[n] = name.rsplit('_',1)[0]
    ms.update({'3':'USB_D_N','4':'USB_D_P','7':'USB_REXT','9':'USB_TX_N','11':'USB_TX_P','13':'USB_RX_N','14':'USB_RX_P','21':'XTIN','22':'XTOUT','26':'HDMI_SCL','27':'HDMI_SDA','28':'HDMI_DET','29':'HDMI_HPD_IC','35':'SPI_MOSI','36':'SPI_CLK','37':'SPI_MISO','38':'SPI_CS','42':'VBUS_DET','43':None,'44':None,'45':'HDMI_REXT','46':'HDMI_C_N','49':'HDMI_C_P','53':'HDMI_D0_N','54':'HDMI_D0_P','57':'HDMI_D1_N','58':'HDMI_D1_P','61':'HDMI_D2_N','62':'HDMI_D2_P','63':None})
    add('U1','MS2130','MS2130',ms,(52,46),sch=(158.75,80.01))
    hd = {1:'HDMI_D2_P',2:'GND',3:'HDMI_D2_N',4:'HDMI_D1_P',5:'GND',6:'HDMI_D1_N',7:'HDMI_D0_P',8:'GND',9:'HDMI_D0_N',10:'HDMI_C_P',11:'GND',12:'HDMI_C_N',13:None,14:None,15:'HDMI_SCL',16:'HDMI_SDA',17:'GND',18:'HDMI_5V',19:'HDMI_HPD',20:'GND'}
    add('J1',gen.HDMI_NAME,'REUSED HDMI-A',hd,(36,46),90,sch=(38.1,80.01),note='Provisional compatible land pattern; verify with 1:1 print before fabrication')
    us = {1:'USB_VBUS',2:'USB_D_N',3:'USB_D_P',4:'GND',5:'USB_CTX_N',6:'USB_CTX_P',7:'GND',8:'USB_RX_N',9:'USB_RX_P','SH':'GND'}
    add('J2','USB3_A_PLUG','USB3 A PLUG',us,(94.75,60),mpn='692112030100',sch=(302.26,50.8))
    add('J3','HEADER2','EXT 5V INPUT',{1:'EXT_5V',2:'GND'},(82,27),sch=(368.3,40.64),note='Regulated 5V only; current-limited bench supply for first bring-up')
    add('JP1','HEADER3','PWR SELECT',{1:'USB_VBUS',2:'SYS_5V',3:'EXT_5V'},(82,35),sch=(368.3,78.74),note='Default shunt 2-3 EXTERNAL. Never fit two shunts. 1-2 USB requires bus-power/inrush validation')
    for ref, val, netin, netout, y, sy in [('U2','XC6220B331MR-G','SYS_5V','P3V3',30,45.72),('U3','XC6220B251MR-G','SYS_5V','P2V5',40,96.52),('U4','XC6220B111MR-G','P2V5','P1V1',50,147.32)]:
        add(ref,'XC6220',val,{1:netin,2:'GND',3:netin,4:None,5:netout},(72,y),sch=(447.04,sy))
    add('U5','SPI_64KB','W25X05CLSNIG',{1:'SPI_CS',2:'SPI_MISO',3:'DVDD33_FLASH',4:'GND',5:'SPI_MOSI',6:'SPI_CLK',7:'DVDD33_FLASH',8:'DVDD33_FLASH'},(56,29),sch=(302.26,121.92),note='64KB firmware/configuration image must be supplied; blank flash is not programmed by this project')
    add('Y1','XTAL','24MHz CL=12pF',{1:'XTIN',2:'GND',3:'XTOUT',4:'GND'},(62,47.5),sch=(266.7,213.36),mpn='24MHz 3225 4-pad crystal, CL=12pF, verify ESR/drive')
    add('J4','HEADER6','SPI PROG (UNPOWERED)',{1:'GND',2:'DVDD33_FLASH',3:'SPI_CS',4:'SPI_CLK',5:'SPI_MOSI',6:'SPI_MISO'},(35,24),angle=90,sch=(337.82,167.64),note='Power off board; do not drive MS2130 SPI bus unless isolated or held inactive. Header is not a power source')
    ritems=[('R1','1.6k 1%','HDMI_REXT','GND',(51,35.5)),('R2','200R 1%','USB_REXT','GND',(47,60.5)),('R3','1M','XTIN','XTOUT',(62,44)),('R4','47k','HDMI_5V','HDMI_SCL',(38,36)),('R5','47k','HDMI_5V','HDMI_SDA',(38,33.5)),('R6','1k','HDMI_5V','HDMI_DET',(45,33.5)),('R7','1k','HDMI_HPD_IC','HDMI_HPD',(45,31)),('R8','24k 1%','USB_VBUS','VBUS_DET',(57,35)),('R9','47k 1%','VBUS_DET','GND',(60,35))]
    for i,(r,v,a,b,xy) in enumerate(ritems):
        add(r,'R',v,{1:a,2:b},xy,sch=(50.8+(i%5)*55.88,160.02+(i//5)*38.1))
    # Separate analog/digital rail branches. 0-ohm links avoid guessing ferrite impedance.
    domains=[('P3V3','DVDD33'),('P3V3','AVDD33_USB'),('P3V3','AVDD33'),('P2V5','AVDD25'),('P1V1','DVDD11'),('P1V1','AVDD11_USB'),('P1V1','AVDD11'),('P3V3','DVDD33_FLASH')]
    for i,(source,target) in enumerate(domains):
        xy=(64+(i%4)*4,59+(i//4)*5)
        add('FB'+str(i+1),'FB','0R / bead option',{1:source,2:target},xy,sch=(368.3+(i%3)*66.04,220.98+(i//3)*27.94),note='Populate 0R first. Ferrite substitution requires transient/stability measurements')
    caps=[]
    # LDO local CIN/COUT: retain manufacturer's 10uF recommendation, X7R DC bias matters.
    for (rail,y) in [('P3V3',30),('P2V5',40),('P1V1',50)]:
        netin='P2V5' if rail=='P1V1' else 'SYS_5V'
        caps += [('10uF 10V X7R',netin,'GND',(68,y-2.5),'F'),('10uF 10V X7R',rail,'GND',(76,y-2.5),'F')]
    caps += [('22pF C0G','XTIN','GND',(59.5,50.5),'F'),('22pF C0G','XTOUT','GND',(64.5,50.5),'F'),('100nF','HDMI_5V','GND',(42,35.5),'F'),('100nF','USB_TX_P','USB_CTX_P',(65,68),'F'),('100nF','USB_TX_N','USB_CTX_N',(65,66),'F')]
    # One bypass capacitor for every individual MS2130 supply pin. Backside rows.
    per_side=defaultdict(list)
    for n,net in ms.items():
        if net and (net.startswith(('AVDD','DVDD')) and 'VSS' not in net):
            per_side[(int(n)-1)//16].append((int(n),net))
    for side,pins in per_side.items():
        for i,(n,net) in enumerate(pins):
            d=(i-(len(pins)-1)/2)*1.9
            if side==0: xy=(52+d,57.5)
            elif side==1: xy=(63.5,46-d)
            elif side==2: xy=(52-d,34.5)
            else: xy=(36,46+(i-(len(pins)-1)/2)*1.6)
            caps.append(('100nF',net,'GND',xy,'B'))
    for i,(_,net) in enumerate(domains):
        caps.append(('1uF 10V X7R',net,'GND',(64+(i%4)*4,61+(i//4)*5),'B'))
    caps.append(('100nF','DVDD33_FLASH','GND',(56,24),'F'))
    for i,(v,a,b,xy,side) in enumerate(caps,1):
        idx=i-1
        sx=30.48+(idx%10)*55.88
        sy=317.5+(idx//10)*17.78
        if idx<6: sx,sy=503.0,30.48+(idx//2)*50.8+(idx%2)*20.32
        angle=90 if side=='B' and xy[1] in (57.5,34.5) else 0
        add('C'+str(i),'C',v,{1:a,2:b},xy,angle=angle,side=side,sch=(sx,sy),note='22pF crystal capacitors are starting values; tune for actual CL and PCB stray' if '22pF' in v else '')
    # Single-channel 0.5pF TVS protection; pin1 I/O (cathode), pin2 ground.
    hsnets=['HDMI_D2_P','HDMI_D2_N','HDMI_D1_P','HDMI_D1_N','HDMI_D0_P','HDMI_D0_N','HDMI_C_P','HDMI_C_N','USB_D_N','USB_D_P','USB_CTX_N','USB_CTX_P','USB_RX_N','USB_RX_P','HDMI_SCL','HDMI_SDA','HDMI_HPD','HDMI_5V']
    for i,net in enumerate(hsnets):
        if i<8: xy=(42.5,51.6-i*1.6)
        elif i<14: xy=(81,55+(i-8)*1.8)
        else: xy=(37,56+(i-14)*2)
        add('D'+str(i+1),'TVS','TPD1E05U06DYAR',{1:net,2:'GND'},xy,0,mpn='TPD1E05U06DYAR',sch=(35.56+(i%9)*60.96,261.62+(i//9)*27.94))
    for i,(net,xy) in enumerate([('SYS_5V',(84,46)),('P3V3',(78,30)),('P2V5',(78,40)),('P1V1',(78,50)),('GND',(84,50)),('VBUS_DET',(64,35)),('HDMI_HPD_IC',(47,28))],1):
        add('TP'+str(i),'TESTPOINT',net,{1:net},xy,sch=(368.3+(i%3)*66.04,170.18+(i//3)*17.78))
    # Flags only on genuinely externally driven/bead-filtered rails; no output-output suppression.
    for i,net in enumerate(['GND','SYS_5V']+[t for _,t in domains]):
        add('#FLG'+str(i+1),'FLAG',net,{1:net},(0,0),sch=(38.1+(i%5)*43.18,223.52+(i//5)*12.7))


def symbol_pins(text):
    return [(num,float(x),float(y),int(ang)) for x,y,ang,num in re.findall(r'\(pin \w+ line \(at ([\d.\-]+) ([\d.\-]+) (\d+)\).*?\(number "([^"]+)"',text,re.S)]


def schematic():
    pieces=[f'(kicad_sch (version 20250114) (generator "eeschema") (uuid {q(ROOT_ID)}) (paper "A2")',
            '(title_block (title "MS2130 HDMI Capture - XC6220 Prototype") (date "2026-10-23") (rev "A0 - ENGINEERING PROTOTYPE") (company "KBM / future") (comment 1 "NOT FABRICATION RELEASE: fit, firmware, thermal and SI validation required"))',
            '(lib_symbols']
    for name,text in DEFS.items():
        pieces.append(text.replace(f'(symbol "{name}"',f'(symbol "TestBoard:{name}"',1))
    pieces.append(')')
    for part in PARTS:
        ref,symbol=part['ref'],part['symbol']
        x,y=part['sch']
        # Everything on the 1.27mm schematic grid, including inherited QFN symbol pins.
        x,y=round(x/1.27)*1.27,round(y/1.27)*1.27
        part['sch']=(x,y)
        fp=re.search(r'\(property "Footprint" "([^"]*)"',DEFS[symbol]).group(1)
        on='no' if symbol=='FLAG' else 'yes'
        bom='no' if symbol in ('FLAG','TESTPOINT') else 'yes'
        pieces.append(f'(symbol (lib_id "TestBoard:{symbol}") (at {x:g} {y:g} 0) (unit 1) (in_bom {bom}) (on_board {on}) (dnp no) (uuid {q(part["uuid"])})')
        ry,vy=(y-35.56,y-33.02) if symbol in ('MS2130',gen.HDMI_NAME) else (y-12.7,y-10.16)
        if symbol in ('R','C','FB','TVS'): ry,vy=y-5.08,y-2.54
        if symbol in ('FLAG','TESTPOINT'): ry,vy=y-5.08,y-2.54
        if symbol=='MS2130': ry,vy=y-1.27,y+1.27
        for prop,val,py,hide in [('Reference',ref,ry,False),('Value',part['value'],vy,False),('Footprint',fp,y,True),('Datasheet','',y,True)]:
            pieces.append(f'(property {q(prop)} {q(val)} (at {x:g} {py:g} 0) (effects (font (size 1 1)){" hide" if hide else ""}))')
        for number,px,py,ang in symbol_pins(DEFS[symbol]):
            pieces.append(f'(pin {q(number)} (uuid {q(uid(ref+":"+number))}))')
        pieces.append(f'(instances (project "{NAME}" (path "/{ROOT_ID}" (reference {q(ref)}) (unit 1)))))')
        for number,px,py,ang in symbol_pins(DEFS[symbol]):
            ax,ay=x+px,y-py
            net=part['nets'].get(number)
            if net is None:
                pieces.append(f'(no_connect (at {ax:g} {ay:g}) (uuid {q(uid(ref+number+"NC"))}))')
                continue
            # Short wire from every pin to its named net, outward from the symbol.
            dx,dy={0:(-5.08,0),180:(5.08,0),90:(0,5.08),270:(0,-5.08)}[ang]
            bx,by=ax+dx,ay+dy
            pieces.append(f'(wire (pts (xy {ax:g} {ay:g}) (xy {bx:g} {by:g})) (stroke (width 0) (type default)) (uuid {q(uid(ref+number+"wire"))}))')
            # Labels stay horizontal; vertical pins get additional right-side offset via wire.
            justify='left' if dx>=0 else 'right'
            la=90 if dx==0 else 0
            pieces.append(f'(label {q(net)} (at {bx:g} {by:g} {la}) (effects (font (size 0.85 0.85)) (justify {justify} bottom)) (uuid {q(uid(ref+number+"label"))}))')
    notes=[(25.4,17.78,'HDMI INPUT / MS2130 / USB 3.0 DEVICE - DRAFT A0'),(355.6,17.78,'REGULATORS: 5V -> 3V3; 5V -> 2V5 -> 1V1'),(25.4,248.92,'LOW-CAPACITANCE TVS PROTECTION (I/O pin 1, GND pin 2)'),(25.4,304.8,'LOCAL BYPASS: ONE 100nF PER MS2130 SUPPLY PIN'),(355.6,213.36,'SEPARATE POWER DOMAINS - FIT 0R FIRST'),(25.4,212.09,'External power / bead-output ERC flags. No waived ERC errors.'),(25.4,135.89,'JP1: 2-3 external 5V DEFAULT; 1-2 USB optional.'),(25.4,142.24,'NEVER bridge both sources. HDMI pin18 is INPUT ONLY.'),(25.4,148.59,'Firmware image, crystal CL and HDMI fit need confirmation.')]
    for i,(x,y,note) in enumerate(notes):
        pieces.append(f'(text {q(note)} (at {x:g} {y:g} 0) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid {q(uid("note"+str(i)))}))')
    pieces.append('(embedded_fonts no))')
    (OUT/(NAME+'.kicad_sch')).write_text('\n'.join(pieces)+'\n',encoding='utf-8')


def board():
    b=pcb.BOARD()
    print('Board initialized', flush=True)
    b.SetCopperLayerCount(4)
    b.GetDesignSettings().SetBoardThickness(pcb.FromMM(1.6))
    nets={}
    for name in sorted({n for p in PARTS for n in p['nets'].values() if n}):
        net=pcb.NETINFO_ITEM(b,name)
        b.Add(net); nets[name]=net
    footprints={}
    for part in PARTS:
        if part['symbol']=='FLAG': continue
        fpname=re.search(r'\(property "Footprint" "TestBoard:([^"]+)"',DEFS[part['symbol']]).group(1)
        fp=pcb.FootprintLoad(str(LIB),fpname)
        assert fp, fpname
        fp.SetReference(part['ref']); fp.SetValue(part['value'])
        fp.SetFPID(pcb.LIB_ID('TestBoard',fpname))
        fp.SetPosition(vec(*part['at']))
        fp.SetOrientationDegrees(part['angle'])
        b.Add(fp)
        if part['side']=='B': fp.Flip(fp.GetPosition(),False)
        fp.SetPath(pcb.KIID_PATH('/'+ROOT_ID+'/'+part['uuid']))
        for pad in fp.Pads():
            name=part['nets'].get(pad.GetNumber())
            if name: pad.SetNet(nets[name])
        # Compact consistent fabrication labels; assembly values stay on Fab.
        fp.Reference().SetTextSize(vec(0.8,0.8)); fp.Reference().SetTextThickness(pcb.FromMM(0.12))
        fp.Reference().SetLayer(pcb.B_Fab if part['side']=='B' else pcb.F_Fab)
        fp.Reference().SetPosition(vec(part['at'][0],part['at'][1]-2.0))
        fp.Reference().SetTextAngle(pcb.EDA_ANGLE(0,pcb.DEGREES_T))
        fp.Value().SetVisible(False)
        for item in list(fp.GraphicalItems()):
            if isinstance(item,pcb.PCB_TEXT):
                item.SetLayer(pcb.B_Fab if part['side']=='B' else pcb.F_Fab)
                item.SetVisible(False)
            elif part['ref']=='J1' and item.GetLayer()==pcb.F_SilkS:
                # The provisional mechanical outline is on Fab; source tabs clip its silk.
                item.SetLayer(pcb.F_Fab)
        if part['ref'].startswith('D'):
            fp.Reference().SetPosition(vec(part['at'][0]-2.5,part['at'][1]))
        footprints[part['ref']]=fp
    # Connector reference edges: HDMI x=25.91, USB plug x=89.0.
    for a,c in [((25.91,20),(89,20)),((89,20),(89,75)),((89,75),(25.91,75)),((25.91,75),(25.91,20))]:
        s=pcb.PCB_SHAPE(); s.SetShape(pcb.SHAPE_T_SEGMENT); s.SetStart(vec(*a)); s.SetEnd(vec(*c)); s.SetLayer(pcb.Edge_Cuts); s.SetWidth(pcb.FromMM(.05)); b.Add(s)
    # Two continuous ground reference layers. Power and all signals stay on outer layers.
    for layer in (pcb.In1_Cu,pcb.In2_Cu):
        z=pcb.ZONE(b); z.SetLayer(layer); z.SetNet(nets['GND']); z.SetLocalClearance(pcb.FromMM(.15)); z.SetThermalReliefGap(pcb.FromMM(.2)); z.SetThermalReliefSpokeWidth(pcb.FromMM(.25)); z.SetPadConnection(pcb.ZONE_CONNECTION_FULL)
        poly=z.Outline(); poly.NewOutline()
        for x,y in [(26.21,20.3),(88.7,20.3),(88.7,74.7),(26.21,74.7)]: poly.Append(pcb.FromMM(x),pcb.FromMM(y))
        b.Add(z)
    for i,(net,fp) in enumerate(footprints.items()):
        # Routed ground escapes are generated separately by the router, not blind vias.
        pass
    for x,y,text,size in [(55,71.5,'MS2130 / XC6220 TEST A0',1),(55,73,'ENGINEERING PROTOTYPE',.8),(81,23,'EXT 5V',.8),(81,43,'JP1 2-3 EXT',.8),(36,65,'HDMI IN',.8)]:
        t=pcb.PCB_TEXT(b); t.SetText(text); t.SetPosition(vec(x,y)); t.SetTextSize(vec(size,size)); t.SetTextThickness(pcb.FromMM(.12)); t.SetLayer(pcb.F_SilkS); b.Add(t)
    power_copper(b)
    # Ground stitching / regulator returns are intentionally the only default routes.
    for x,y in [(67,23),(79,23),(67,53),(79,53)]+[(52+dx,46+dy) for dx in (-1.6,0,1.6) for dy in (-1.6,0,1.6)]:
        via=pcb.PCB_VIA(b); via.SetPosition(vec(x,y)); via.SetWidth(pcb.FromMM(.6)); via.SetDrill(pcb.FromMM(.3)); via.SetViaType(pcb.VIATYPE_THROUGH); via.SetLayerPair(pcb.F_Cu,pcb.B_Cu); via.SetNet(nets['GND']); b.Add(via)
    for y in (30,40,50):
        a=(70.8625,y); c=(69.8,y)
        via=pcb.PCB_VIA(b); via.SetPosition(vec(*c)); via.SetWidth(pcb.FromMM(.6)); via.SetDrill(pcb.FromMM(.3)); via.SetViaType(pcb.VIATYPE_THROUGH); via.SetLayerPair(pcb.F_Cu,pcb.B_Cu); via.SetNet(nets['GND']); b.Add(via)
        track=pcb.PCB_TRACK(b); track.SetStart(vec(*a)); track.SetEnd(vec(*c)); track.SetWidth(pcb.FromMM(.3)); track.SetLayer(pcb.F_Cu); track.SetNet(nets['GND']); b.Add(track)
    pcb.SaveBoard(str(OUT/(NAME+'.kicad_pcb')),b)
    return b


def power_copper(b):
    # Copper heat spreading is provisional, not a substitute for a thermal test.
    nets={net.GetNetname():net for net in b.GetNetsByNetcode().values()}
    for layer,netname,rect in [(pcb.F_Cu,'SYS_5V',(64,22,80,43)),(pcb.F_Cu,'P2V5',(70,45,74,54)),(pcb.B_Cu,'GND',(66,22,80,54))]:
        z=pcb.ZONE(b); z.SetLayer(layer); z.SetNet(nets[netname]); z.SetLocalClearance(pcb.FromMM(.15)); z.SetPadConnection(pcb.ZONE_CONNECTION_FULL)
        poly=z.Outline(); poly.NewOutline(); x1,y1,x2,y2=rect
        for x,y in [(x1,y1),(x2,y1),(x2,y2),(x1,y2)]: poly.Append(pcb.FromMM(x),pcb.FromMM(y))
        b.Add(z)


def project():
    settings={"meta":{"filename":NAME+'.kicad_pro',"version":1},"board":{"design_settings":{"rules":{"min_clearance":0.127,"min_track_width":0.127,"min_via_diameter":0.5,"min_through_hole_diameter":0.25,"min_hole_clearance":0.2,"min_copper_edge_clearance":0.25},"rule_severities":{}}},"net_settings":{"classes":[{"name":"Default","clearance":0.127,"track_width":0.18,"via_diameter":0.55,"via_drill":0.3,"diff_pair_width":0.18,"diff_pair_gap":0.18,"diff_pair_via_gap":0.25}],"meta":{"version":4}},"schematic":{"erc":{"pin_map":[]}}}
    (OUT/(NAME+'.kicad_pro')).write_text(json.dumps(settings,indent=2)+'\n',encoding='utf-8')
    (OUT/(NAME+'.kicad_dru')).write_text('(version 1)\n(rule "Minimum prototype geometry" (constraint clearance (min 0.127mm)) (constraint track_width (min 0.127mm)))\n',encoding='utf-8')
    (OUT/'design_manifest.json').write_text(json.dumps({'revision':'A0','status':'ENGINEERING_PROTOTYPE_NOT_RELEASED','board_mm':[63.09,55.0],'layers':['F.Cu signals/power','In1.Cu GND','In2.Cu GND','B.Cu signals/power'],'parts':PARTS},indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
    with (OUT/'BOM.csv').open('w',encoding='utf-8-sig',newline='') as f:
        w=csv.writer(f); w.writerow(['Reference','Value','MPN_or_specification','Side','Assembly_note'])
        for p in PARTS:
            if p['symbol'] not in ('FLAG','TESTPOINT'): w.writerow([p['ref'],p['value'],p['mpn'],p['side'],p['note']])


if __name__=='__main__':
    setup_libs(); parts(); schematic(); project(); board()
    print(f'Created {OUT}: {sum(p["symbol"]!="FLAG" for p in PARTS)} components. Routing/DRC still required.')
