"""Native schematic-netlist to PCB-pad parity and critical-connection checks."""
from __future__ import annotations
import json
import re
import sys
import zipfile
from pathlib import Path
import pcbnew as p

ROOT=Path(__file__).resolve().parents[1]/'boards'/'ms2130-test'


def parse(text):
    tokens=re.findall(r'"(?:\\.|[^"\\])*"|[()]|[^\s()]+',text)
    stack=[]; result=None
    for token in tokens:
        if token=='(':
            node=[]
            if stack: stack[-1].append(node)
            stack.append(node)
        elif token==')': result=stack.pop()
        else: stack[-1].append(json.loads(token) if token.startswith('"') else token)
    assert not stack
    return result


def child(node,key):
    return next(a for a in node if isinstance(a,list) and a and a[0]==key)


def main():
    manifest=json.loads((ROOT/'design_manifest.json').read_text(encoding='utf-8'))
    expected={(x['ref'],pin):net for x in manifest['parts'] if x['symbol']!='FLAG' for pin,net in x['nets'].items() if net}
    native=parse((ROOT/'review'/'netlist.net').read_text(encoding='utf-8'))
    actual={}
    for net in child(native,'nets')[1:]:
        name=child(net,'name')[1].lstrip('/')
        for node in net:
            if not isinstance(node,list) or node[0]!='node': continue
            ref,pin=child(node,'ref')[1],child(node,'pin')[1]
            if not ref.startswith('#') and not name.startswith('unconnected-'): actual[(ref,pin)]=name
    assert expected==actual, f'Native schematic netlist mismatch: {[(k,v,actual.get(k)) for k,v in expected.items() if actual.get(k)!=v]}'
    board=p.LoadBoard(str(ROOT/'MS2130_Test.kicad_pcb'))
    if '--sync-native-names' in sys.argv:
        existing={net.GetNetname().lstrip('/'):net for net in board.GetNetsByNetcode().values()}
        lookup={(fp.GetReference(),pad.GetNumber()):pad for fp in board.GetFootprints() for pad in fp.Pads() if pad.GetNumber()}
        # Several physical shell tabs share one pin. Rename existing net objects,
        # rather than rebinding a single physical pad of a duplicated shell pin.
        for node in child(native,'nets')[1:]:
            name=child(node,'name')[1]
            short=name.lstrip('/')
            if short in existing:
                existing[short].SetNetname(name)
            elif name.startswith('unconnected-'):
                n=p.NETINFO_ITEM(board,name); board.Add(n)
                for item in node:
                    if isinstance(item,list) and item[0]=='node':
                        key=(child(item,'ref')[1],child(item,'pin')[1])
                        if key in lookup: lookup[key].SetNet(n)
        p.SaveBoard(str(ROOT/'MS2130_Test.kicad_pcb'),board)
    native_pads={(fp.GetReference(),pad.GetNumber()):pad.GetNetname().lstrip('/') for fp in board.GetFootprints() for pad in fp.Pads() if pad.GetNumber() and pad.GetNetCode() and not pad.GetNetname().startswith('unconnected-')}
    assert expected==native_pads, f'PCB-pad net mismatch: {set(expected.items()) ^ set(native_pads.items())}'
    critical={('U1','46'):'HDMI_C_N',('U1','50'):'GND',('U1','35'):'SPI_MOSI',('U1','36'):'SPI_CLK',('U1','37'):'SPI_MISO',('U1','38'):'SPI_CS',('J2','5'):'USB_CTX_N',('J2','6'):'USB_CTX_P',('J2','8'):'USB_RX_N',('J2','9'):'USB_RX_P',('U4','1'):'P2V5',('U4','5'):'P1V1',('J1','18'):'HDMI_5V',('JP1','1'):'USB_VBUS',('JP1','2'):'SYS_5V',('JP1','3'):'EXT_5V'}
    for key,value in critical.items(): assert actual[key]==value,(key,value)
    assert board.GetCopperLayerCount()==4
    shells=[pad for fp in board.GetFootprints() if fp.GetReference()=='J1' for pad in fp.Pads() if pad.GetNumber()=='20']
    assert len(shells)==4 and all(pad.GetNetname().lstrip('/')=='GND' for pad in shells)
    lines=[f'KiCad native parser {p.Version()}',f'PASS: {len(expected)} connected pin identities match manifest, native schematic netlist and PCB',f'PASS: {len(critical)} independently asserted critical connections','PASS: USB-A plug host-RX contacts carry MS2130 TX; host-TX contacts carry MS2130 RX','PASS: 1.1V regulator input is 2.5V, not 5V','PASS: HDMI pin18 is not connected to board 5V; four shell tabs share GND','PASS: Four copper layers','NOTE: ERC/DRC reports are separate. This check is not firmware, SI, thermal or physical-fit validation.']
    (ROOT/'review'/'NETLIST_PARITY.txt').write_text('\n'.join(lines)+'\n',encoding='utf-8')
    print('\n'.join(lines))
    if '--summarize' in sys.argv:
        erc=(ROOT/'review'/'ERC.rpt').read_text(encoding='utf-8')
        drc=(ROOT/'review'/'DRC-final.rpt').read_text(encoding='utf-8')
        e=re.search(r'ERC messages: (\d+)\s+Errors (\d+)\s+Warnings (\d+)',erc)
        d=re.search(r'Found (\d+) DRC violations',drc)
        u=re.search(r'Found (\d+) unconnected pads',drc)
        f=re.search(r'Found (\d+) Footprint errors',drc)
        assert e and d and u
        state={'schematic_erc_errors':int(e[2]),'schematic_erc_warnings':int(e[3]),'pcb_geometry_violations':int(d[1]),'pcb_unconnected_items':int(u[1]),'pcb_schematic_parity_issues':int(f[1]) if f else 0,'pin_identity_checks':len(expected),'fabrication_release':False,'status':'SCHEMATIC_AND_PLACEMENT_DRAFT_ROUTING_INCOMPLETE'}
        text='CURRENT CHECKS - MS2130 Test A0 - NOT FOR FABRICATION\n\n'+'\n'.join(f'{k}: {v}' for k,v in state.items())+'\n\nPCB signal and power routing is incomplete. Geometry checks passing do NOT\nmean that all nets are physically connected. See DRC-final.rpt.\nExperimental router output is not the default project.\nNo physical assembly, firmware programming, SI or thermal testing was performed.\n'
        (ROOT/'review'/'CHECKS.txt').write_text(text,encoding='utf-8')
        (ROOT/'review'/'CHECKS.json').write_text(json.dumps(state,indent=2)+'\n',encoding='utf-8')
        print(text)
    if '--package' in sys.argv:
        target=ROOT.parent/'MS2130_Test_A0_DRAFT.zip'
        top='MS2130_Test_A0_DRAFT/'
        rootnames=['MS2130_Test.kicad_pro','MS2130_Test.kicad_sch','MS2130_Test.kicad_pcb','MS2130_Test.kicad_dru','TestBoard.kicad_sym','sym-lib-table','fp-lib-table','BOM.csv','design_manifest.json','DESIGN_STATUS.txt']
        reviewnames=['ERC.rpt','DRC-final.rpt','NETLIST_PARITY.txt','CHECKS.txt','CHECKS.json','MS2130_Test.svg','schematic-preview.png','pcb-placement.svg','pcb-placement-preview.png']
        files=[ROOT/n for n in rootnames]+[ROOT/'review'/n for n in reviewnames]+list((ROOT/'TestBoard.pretty').glob('*.kicad_mod'))
        with zipfile.ZipFile(target,'w',zipfile.ZIP_DEFLATED) as z:
            for path in sorted(files):
                assert path.is_file(),path
                z.write(path,top+path.relative_to(ROOT).as_posix())
            future=ROOT.parents[1]
            for source in [future/'references'/'MS2130-datasheet.pdf',future/'references'/'HDMI_A_Foxconn_QJ5119x-WFD6_drawing.pdf',future/'docs'/'HDMI_A_REUSED_fit_1to1.svg']:
                assert source.is_file(),source
                z.write(source,top+'references/'+source.name)
        with zipfile.ZipFile(target) as z: assert z.testzip() is None
        print(f'Draft package validated: {target} ({target.stat().st_size} bytes)')


if __name__=='__main__': main()
