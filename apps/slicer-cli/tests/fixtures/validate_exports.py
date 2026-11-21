# Copyright (c) 2026 Sleptic
# SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
"""Independent structural readers for R23 exports (PNG, ZIP, SVG and 3MF).

Uses only Python's standard library and never imports slicer writer code.
PNG: ISO/IEC 15948 chunk framing, CRC, IHDR, zlib and scanline filters.
SVG and 3MF: XML container structure, geometry dimensions and layer ordering.
This is file validation, not a firmware simulator.
"""
import argparse
import json
import re
import struct
import xml.etree.ElementTree as ET
import zipfile
import zlib
from pathlib import Path


def png(data):
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "PNG signature"
    offset, compressed, header = 8, bytearray(), None
    ended = False
    while offset < len(data):
        length = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset+4:offset+8]
        payload = data[offset+8:offset+8+length]
        assert len(payload) == length, "truncated PNG chunk"
        crc = struct.unpack_from(">I", data, offset+8+length)[0]
        assert zlib.crc32(kind+payload) & 0xffffffff == crc, "PNG CRC"
        if kind == b"IHDR":
            assert header is None and offset == 8 and length == 13
            header = struct.unpack(">IIBBBBB", payload)
        elif kind == b"IDAT":
            compressed.extend(payload)
        elif kind == b"IEND":
            ended = True
            break
        offset += length+12
    assert ended and header, "PNG mandatory chunks"
    width, height, depth, color, compression, filter_method, interlace = header
    assert width and height and depth == 8 and color in (0, 2, 6)
    assert (compression, filter_method, interlace) == (0, 0, 0)
    channels = {0: 1, 2: 3, 6: 4}[color]
    stride = width*channels+1
    stream = zlib.decompressobj()
    decoded = stream.decompress(compressed)+stream.flush()
    assert stream.eof and not stream.unused_data and len(decoded) == stride*height
    assert all(decoded[y*stride] <= 4 for y in range(height)), "PNG filter"
    return width, height


def script(text, images, process=False):
    assert "{" not in text and "}" not in text, "unresolved placeholder"
    layers, heights, displayed = [], [], []
    for number, line in enumerate(text.splitlines(), 1):
        if line.startswith(";LAYER_START:"):
            layers.append(int(line.split(":", 1)[1]))
        if line.startswith(";Z:"):
            heights.append(float(line.split(":", 1)[1]))
        command = line.split(";", 1)[0].strip()
        if not command:
            continue
        assert re.fullmatch(r"[GMT]\d+(?:\s+[^{};]+)?", command), (number, command)
        if command.startswith("M6054 "):
            match = re.fullmatch(r'M6054\s+"([^"{}]+)"', command)
            assert match, (number, command)
            displayed.append(match[1])
    assert len(layers) == len(images) == len(heights), "script/image layer count"
    assert len(set(layers)) == len(layers) and all(a < b for a, b in zip(layers, layers[1:]))
    assert all(a < b for a, b in zip(heights, heights[1:])), "non-monotonic print Z"
    assert set(displayed) == set(images), "displayed images"
    if process:
        assert all(abs((b-a)-0.05)<1e-6 for a,b in zip(heights,heights[1:])), "layer height"
        blocks=text.split(";LAYER_START:")[1:]
        for index, block in enumerate(blocks):
            body=block.split(";LAYER_END",1)[0].splitlines()
            moves=[line.split(";",1)[0].strip() for line in body if line.startswith("G1 ")]
            assert len(moves)==3, "position/lift/retract moves"
            values=[]
            for move in moves:
                match=re.fullmatch(r"G1 Z([-+0-9.eE]+) F([-+0-9.eE]+)",move)
                assert match, move
                values.append(tuple(map(float,match.groups())))
            assert abs(values[0][0]-heights[index])<1e-6 and values[0][1]==150.0
            assert values[1:]==[(6.0,60.0),(-6.0,150.0)], "lift/retract"
            assert any(line.startswith("G91") for line in body) and any(line.startswith("G90") for line in body)
            exposures=[line for line in body if "exposure dwell" in line]
            assert len(exposures)==1
            milliseconds=float(re.search(r"G4 P([-+0-9.eE]+)",exposures[0]).group(1))
            assert milliseconds==(30000.0 if index<5 else 2500.0), "exposure/bottom layers"
    return len(layers)


def validate(path, process=False):
    path = Path(path)
    result = {"path": str(path)}
    if path.is_dir():
        files = sorted(path.iterdir())
        pngs = [p for p in files if p.suffix == ".png"]
        svgs = [p for p in files if p.suffix == ".svg"]
        assert pngs or svgs, "no layers"
        dimensions = [png(p.read_bytes()) for p in pngs]
        assert not dimensions or set(dimensions) == {(3840, 2400)}
        for p in svgs:
            svg(p.read_bytes())
        result.update(layers=len(pngs or svgs), format="png-per-layer" if pngs else "svg-per-layer")
    elif zipfile.is_zipfile(path):
        with zipfile.ZipFile(path) as archive:
            assert archive.testzip() is None, "ZIP CRC"
            names = archive.namelist()
            if "run.gcode" in names:
                images = [n for n in names if re.fullmatch(r"\d+\.png", n)]
                assert images and all(png(archive.read(n)) == (3840, 2400) for n in images)
                layers = script(archive.read("run.gcode").decode("utf-8"), images, process)
                assert png(archive.read("preview.png")) == (256, 256)
                result.update(format="gcode-zip", layers=layers, preview="256x256")
            else:
                ET.fromstring(archive.read("[Content_Types].xml"))
                rels = ET.fromstring(archive.read("_rels/.rels"))
                assert any(e.attrib.get("Type", "").endswith("/3dmodel") for e in rels)
                layers = 0
                for name in names:
                    if name.endswith(".model"):
                        root = ET.fromstring(archive.read(name))
                        for stack in root.iter():
                            if stack.tag.endswith("}slicestack"):
                                heights = [float(e.attrib["ztop"]) for e in stack if "ztop" in e.attrib]
                                assert all(a < b for a, b in zip(heights, heights[1:]))
                                layers = max(layers, len(heights))
                if "Metadata/thumbnail.png" in names:
                    assert png(archive.read("Metadata/thumbnail.png")) == (256, 256)
                result.update(format="3mf", layers=layers)
    else:
        result.update(format="svg-combined", layers=svg(path.read_bytes()))
    return result


def svg(data):
    root = ET.fromstring(data)
    assert root.tag == "{http://www.w3.org/2000/svg}svg"
    box = list(map(float, root.attrib["viewBox"].split()))
    assert box[2:] == [192.0, 120.0], "SVG machine dimensions"
    groups = [e for e in root.iter() if "data-layer-index" in e.attrib]
    assert groups
    heights = [float(e.attrib["data-z-mm"]) for e in groups]
    assert all(a < b for a, b in zip(heights, heights[1:]))
    return len(groups)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--standard-process", action="store_true", help="check Sleptic DIY + unmodified standard-405 process values")
    parser.add_argument("files", nargs="+")
    args = parser.parse_args()
    for filename in args.files:
        print(json.dumps(validate(filename,args.standard_process)))
