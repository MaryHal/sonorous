// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Bitmap font.

use std::{int, uint, vec};
use util::gfx::*;
use gl = opengles::gl2;
use glutil = util::gl;

/// Intrinsic width of the bitmap font.
pub static NCOLUMNS: uint = 8;

/// Intrinsic height of the bitmap font.
pub static NROWS: uint = 16;

/// Bit vector which represents one row of zoomed font.
type ZoomedFontRow = u32;

/**
 * A polygon that makes the font glyph up.
 *
 * Specifically: It is a (possibly) concave polygon with corners `(x11,y1)`, `(x12,y1)`,
 * `(xm2-e2,ym)`, `(x22,y2)`, `(x21,y2)` and `(xm1+e1,ym)`. `ym` is an avarage of `y1` and `y2`.
 * `e1` is zero if `x11 == xm1 == x21` or a zoom-independent small bias (typically 1/8th of a pixel)
 * otherwise, and `e2` is defined in the same way. All coordinates have a unit of half-pixel, and
 * the associated colors are automatically calculated from y coordinates.
 */
struct FontPolygon {
    x11: int, x12: int, y1: int,
    xm1: int, xm2: int,
    x21: int, x22: int, y2: int,
}

/// 8x16 resizable bitmap font.
pub struct Font {
    /**
     * Font data used for zoomed font reconstruction. This is actually an array of `u32`
     * elements, where the first `u16` element forms upper 16 bits and the second forms lower
     * 16 bits. It is reinterpreted for better compression.
     *
     * One glyph has 16 `u32` elements for each row from the top to the bottom. One `u32`
     * element contains eight four-bit groups for each column from the left (lowermost group)
     * to the right (uppermost group). Each group is a bitwise OR of following bits:
     *
     * - 1: the lower right triangle of the zoomed pixel should be drawn.
     * - 2: the lower left triangle of the zoomed pixel should be drawn.
     * - 4: the upper right triangle of the zoomed pixel should be drawn.
     * - 8: the upper left triangle of the zoomed pixel should be drawn.
     *
     * So for example, if the group bits read 3 (1+2), the zoomed pixel would be drawn
     * as follows (in the zoom factor 5):
     *
     *     .....
     *     #...#
     *     ##.##
     *     #####
     *     #####
     *
     * The group bits 15 (1+2+4+8) always draw the whole square, so in the zoom factor 1 only
     * pixels with group bits 15 will be drawn.
     */
    glyphs: ~[u16],

    /// Precalculated zoomed font per zoom factor. It is three-dimensional array which indices
    /// are zoom factor, glyph number and row respectively. Assumes that each element has
    /// at least zoom factor times 8 (columns per row) bits.
    pixels: ~[~[~[ZoomedFontRow]]],

    /// Precalculated polygons for glyphs.
    polygons: ~[~[FontPolygon]],
}

/// An alignment mode of `Font::print_string`.
pub enum Alignment {
    /// Coordinates specify the top-left corner of the bounding box.
    LeftAligned,
    /// Coordinates specify the top-center point of the bounding box.
    Centered,
    /// Coordinates specify the top-right corner of the bounding box.
    RightAligned
}

/// Decompresses a bitmap font data. `Font::create_zoomed_font` is required for the actual use.
pub fn Font() -> Font {
    // Delta-coded code words.
    let dwords = [0, 2, 6, 2, 5, 32, 96, 97, 15, 497, 15, 1521, 15, 1537,
        16, 48, 176, 1, 3, 1, 3, 7, 1, 4080, 4096, 3, 1, 8, 3, 4097, 4080,
        16, 16128, 240, 1, 2, 9, 3, 8177, 15, 16385, 240, 15, 1, 47, 721,
        143, 2673, 2, 6, 7, 1, 31, 17, 16, 63, 64, 33, 0, 1, 2, 1, 8, 3];

    // LZ77-compressed indices to code words:
    // - Byte 33..97 encodes a literal code word 0..64;
    // - Byte 98..126 encodes an LZ77 length distance pair with length 3..31;
    //   the following byte 33..126 encodes a distance 1..94.
    let indices =
        ~"!!7a/&/&s$7a!f!'M*Q*Qc$(O&J!!&J&Jc(e!2Q2Qc$-Bg2m!2bB[Q7Q2[e&2Q!Qi>&!&!>UT2T2&2>WT!c*\
          T2GWc8icM2U2D!.8(M$UQCQ-jab!'U*2*2*2TXbZ252>9ZWk@*!*!*8(J$JlWi@cxQ!Q!d$#Q'O*?k@e2dfe\
          jcNl!&JTLTLG_&J>]c*&Jm@cB&J&J7[e(o>pJM$Qs<7[{Zj`Jm40!3!.8(M$U!C!-oR>UQ2U2]2a9Y[S[QCQ\
          2GWk@*M*Q*B*!*!g$aQs`G8.M(U$[!Ca[o@Q2Q!IJQ!Q!c,GWk@787M6U2C2d!a[2!2k?!bnc32>[u`>Uc4d\
          @b(q@abXU!D!.8(J&J&d$q`Q2IXu`g@Q2aWQ!q@!!ktk,x@M$Qk@3!.8(M$U!H#W'O,?4m_f!7[i&n!:eX5g\
          hCk=>UQ2Q2U2Dc>J!!&J&b&k@J)LKg!GK!)7Wk@'8,M=UWCcfa[c&Q2l`f4If(Q2G[l@MSUQC!2!2c$Q:RWG\
          Ok@,[<2WfZQ2U2D2.l`a[eZ7f(!2b2|@b$j!>MSUQCc6[2W2Q:RWGOk@Q2Q2c$a[g*Ql`7[&J&Jk$7[l`!Qi\
          $d^GWk@U2D2.9([$[#['[,@<2W2k@!2!2m$a[l`:^[a[a[T2Td~c$k@d2:R[V[a@_b|o@,M=UWCgZU:EW.Ok\
          @>[g<G[!2!2d$k@Ug@Q2V2a2IW_!Wt`Ih*q`!2>WQ!Q!c,Gk_!7[&J&Jm$k@gti$m`k:U:EW.O(?s@T2Tb$a\
          [CW2Qk@M+U:^[GbX,M>U`[WCO-l@'U,D<.W(O&J&Je$k@a[Q!U!]!G8.M(U$[!Ca[k@*Q!Q!l$b2m!+!:#W'\
          O,?4!1n;c`*!*!l$h`'8,M=UWCO-pWz!a[i,#Q'O,?4~R>QQ!Q!aUQ2Q2Q2aWl=2!2!2>[e<c$G[p`dZcHd@\
          l`czi|c$al@i`b:[!2Un`>8TJTJ&J7[&b&e$o`i~aWQ!c(hd2!2!2>[g@e$k]epi|e0i!bph(d$dbGWhA2!2\
          U2D2.9(['[,@<2W2k`*J*?*!*!k$o!;[a[T2T2c$c~o@>[c6i$p@Uk>GW}`G[!2!2b$h!al`aWQ!Q!Qp`fVl\
          Zf@UWb6>eX:GWk<&J&J7[c&&JTJTb$G?o`c~i$m`k@U:EW.O(v`T2Tb$a[Fp`M+eZ,M=UWCO-u`Q:RWGO.A(\
          M$U!Ck@a[]!G8.M(U$[!Ca[i:78&J&Jc$%[g*7?e<g0w$cD#iVAg*$[g~dB]NaaPGft~!f!7[.W(O";

    /// Decompresses a font data from `dwords` and `indices`.
    fn decompress(dwords: &[u16], indices: &str) -> ~[u16] {
        let mut words = ~[0];
        for dwords.iter().advance |&delta| {
            let last = *words.last();
            words.push(last + delta);
        }

        let nindices = indices.len();
        let mut i = 0;
        let mut glyphs = ~[];
        while i < nindices {
            let code = indices[i] as uint;
            i += 1;
            match code {
                33..97 => { glyphs.push(words[code - 33]); }
                98..126 => {
                    let length = code - 95; // code=98 -> length=3
                    let distance = indices[i] as uint - 32;
                    i += 1;
                    let start = glyphs.len() - distance;
                    for uint::range(start, start + length) |i| {
                        glyphs.push(glyphs[i]);
                    }
                }
                _ => fail!(~"unexpected codeword")
            }
        }
        glyphs
    }

    /// Calculates polygons for given glyph data.
    fn calculate_polygons(rows: &[u16], width: uint) -> ~[FontPolygon] {
        assert!(rows.len() % 2 == 0);

        let mut polygons = ~[];
        let mut y = 0;
        while y < rows.len() {
            let mut data = (rows[y] as u32 << 16) | (rows[y+1] as u32);
            let y1 = y as int;
            y += 2;

            // optimization: if the row only consists of fully filled pixels, and the subsequent
            // rows are identical, then we treat them as one row.
            if (data & 0x11111111) * 0xf == data {
                while y < rows.len() && data == (rows[y] as u32 << 16) | (rows[y+1] as u32) {
                    y += 2;
                }
            }
            let y2 = y as int;

            // the algorithm operates a per-row basis, and extracts runs of valid `FontPolygon`s
            // from given row data. (polygons may overlap, this is fine for our purpose.)
            let mut cur: Option<FontPolygon> = None;
            for int::range_step(0, (width + 1) * 2 as int, 2) |x| { // with a sentinel
                let mut v = data & 15;
                data >>= 4;

                if v & (1|8) == (1|8) || v & (2|4) == (2|4) { // completely filled
                    if cur.is_some() {
                        let mut polygon = cur.swap_unwrap();
                        polygon.x12 += 2;
                        polygon.xm2 += 2;
                        polygon.x22 += 2;
                        cur = Some(polygon);
                    } else {
                        cur = Some(FontPolygon { x11: x, x12: x + 2, y1: y1,
                                                 xm1: x, xm2: x + 2,
                                                 x21: x, x22: x + 2, y2: y2 });
                    }
                } else {
                    if v & (2|8) != 0 && cur.is_some() { // has left-side edge
                        let dx12 = if v & 8 != 0 {2} else {0};
                        let dxm2 = 1;
                        let dx22 = if v & 2 != 0 {2} else {0};
                        if cur.is_some() {
                            let mut polygon = cur.swap_unwrap();
                            polygon.x12 += dx12;
                            polygon.xm2 += dxm2;
                            polygon.x22 += dx22;
                            polygons.push(polygon);
                        } else {
                            // this polygon can't connect to the right side anyway,
                            // so flush immediately.
                            polygons.push(FontPolygon { x11: x, x12: x + dx12, y1: y1,
                                                        xm1: x, xm2: x + dxm2,
                                                        x21: x, x22: x + dx22, y2: y2 });
                        }
                        v &= !(2|8);
                    }

                    // now we have cleared the left side, any remaining polygon should be flushed.
                    if cur.is_some() {
                        polygons.push(cur.swap_unwrap());
                    }

                    if v & (1|4) != 0 { // has right-side edge, add a new polygon
                        let dx11 = if v & 4 != 0 {0} else {2};
                        let dxm1 = 1;
                        let dx21 = if v & 1 != 0 {0} else {2};
                        cur = Some(FontPolygon { x11: x + dx11, x12: x + 2, y1: y1,
                                                 xm1: x + dxm1, xm2: x + 2,
                                                 x21: x + dx21, x22: x + 2, y2: y2 });
                    }
                }
            }
        }

        polygons
    }

    let glyphs = decompress(dwords, indices);
    assert!(glyphs.len() == 3072);

    let mut polygons = ~[];
    for uint::range_step(0, glyphs.len(), NROWS * 2 as int) |base| {
        polygons.push(calculate_polygons(glyphs.slice(base, base + NROWS * 2), NCOLUMNS));
    }

    Font { glyphs: glyphs, pixels: ~[], polygons: polygons }
}

impl Font {
    /// Creates a zoomed font of scale `zoom`.
    pub fn create_zoomed_font(&mut self, zoom: uint) {
        assert!(zoom > 0);
        assert!(zoom <= (8 * ::std::sys::size_of::<ZoomedFontRow>()) / NCOLUMNS);
        if zoom < self.pixels.len() && !self.pixels[zoom].is_empty() { return; }

        let nglyphs = self.glyphs.len() / (NROWS * 2);
        let mut pixels = vec::from_elem(nglyphs, vec::from_elem(zoom * NROWS, 0));

        let put_zoomed_pixel = |glyph: uint, row: uint, col: uint, v: u32| {
            let zoomrow = row * zoom;
            let zoomcol = col * zoom;
            for uint::range(0, zoom) |r| {
                for uint::range(0, zoom) |c| {
                    let mut mask = 0;
                    if r + c >= zoom    { mask |= 1; } // lower right
                    if r > c            { mask |= 2; } // lower left
                    if r < c            { mask |= 4; } // upper right
                    if r + c < zoom - 1 { mask |= 8; } // upper left

                    // if `zoom` is odd, drawing four corner triangles leaves one center pixel
                    // intact since we don't draw diagonals for aesthetic reason. such case
                    // must be specially handled.
                    if (v & mask) != 0 || v == 15 {
                        pixels[glyph][zoomrow+r] |= 1 << (zoomcol+c);
                    }
                }
            }
        };

        let mut i = 0;
        for uint::range(0, nglyphs) |glyph| {
            for uint::range(0, NROWS) |row| {
                let data = (self.glyphs[i] as u32 << 16) | (self.glyphs[i+1] as u32);
                i += 2;
                for uint::range(0, NCOLUMNS) |col| {
                    let v = (data >> (4 * col)) & 15;
                    put_zoomed_pixel(glyph, row, col, v);
                }
            }
        }
        self.pixels.grow_set(zoom, &~[], pixels);
    }

    /// Prints a glyph with given position and color (possibly gradient). This method is
    /// distinct from `print_glyph` since the glyph #95 is used for the tick marker
    /// (character code -1 in C).
    pub fn print_glyph<ColorT:Blend+Copy>(&self, pixels: &mut SurfacePixels, x: uint, y: uint,
                                          zoom: uint, glyph: uint, color: ColorT) { // XXX #3984
        assert!(!self.pixels[zoom].is_empty());
        for uint::range(0, NROWS * zoom) |iy| {
            let row = self.pixels[zoom][glyph][iy];
            let rowcolor = color.blend(iy as int, NROWS * zoom as int);
            for uint::range(0, 8 * zoom) |ix| {
                if ((row >> ix) & 1) != 0 {
                    put_pixel(pixels, x + ix, y + iy, rowcolor); // XXX incorrect lifetime
                }
            }
        }
    }

    /// Draws a glyph with given position and color (possibly gradient). This method is
    /// distinct from `draw_glyph` since the glyph #95 is used for the tick marker
    /// (character code -1 in C).
    pub fn draw_glyph<ColorT:Blend+Copy>(&self, d: &mut glutil::ShadedDrawing, x: f32, y: f32,
                                         zoom: f32, glyph: uint, color: ColorT) { // XXX #3984
        assert!(zoom > 0.0);
        assert!(d.prim == gl::TRIANGLES);
        let zoom = zoom * 0.5;
        for self.polygons[glyph].iter().advance |&polygon| {
            let flat1 = (polygon.x11 == polygon.xm1 && polygon.xm1 == polygon.x21);
            let flat2 = (polygon.x12 == polygon.xm2 && polygon.xm2 == polygon.x22);
            let x11 = x + polygon.x11 as f32 * zoom;
            let x12 = x + polygon.x12 as f32 * zoom;
            let x21 = x + polygon.x21 as f32 * zoom;
            let x22 = x + polygon.x22 as f32 * zoom;
            let y1 = y + polygon.y1 as f32 * zoom;
            let y2 = y + polygon.y2 as f32 * zoom;
            let cy1 = to_rgba(color.blend(polygon.y1, NROWS * 2 as int));
            let cy2 = to_rgba(color.blend(polygon.y2, NROWS * 2 as int));
            if flat1 && flat2 {
                d.point_rgba(x11,y1,cy1); d.point_rgba(x12,y1,cy1); d.point_rgba(x22,y2,cy2);
                d.point_rgba(x11,y1,cy1); d.point_rgba(x22,y2,cy2); d.point_rgba(x21,y2,cy2);
            } else {
                let ym_ = (polygon.y1 + polygon.y2) / 2;
                let ym = y + ym_ as f32 * zoom;
                let cym = to_rgba(color.blend(ym_, NROWS * 2 as int));
                if flat1 {
                    assert!(!flat2);
                    let xm2 = x + polygon.xm2 as f32 * zoom - 0.125;
                    d.point_rgba(x11,y1,cy1); d.point_rgba(x12,y1,cy1); d.point_rgba(xm2,ym,cym);
                    d.point_rgba(x11,y1,cy1); d.point_rgba(xm2,ym,cym); d.point_rgba(x21,y2,cy2);
                    d.point_rgba(xm2,ym,cym); d.point_rgba(x22,y2,cy2); d.point_rgba(x21,y2,cy2);
                } else if flat2 {
                    let xm1 = x + polygon.xm1 as f32 * zoom + 0.125;
                    d.point_rgba(x11,y1,cy1); d.point_rgba(x12,y1,cy1); d.point_rgba(xm1,ym,cym);
                    d.point_rgba(x12,y1,cy1); d.point_rgba(x22,y2,cy2); d.point_rgba(xm1,ym,cym);
                    d.point_rgba(xm1,ym,cym); d.point_rgba(x22,y2,cy2); d.point_rgba(x21,y2,cy2);
                } else {
                    let xm1 = x + polygon.xm1 as f32 * zoom + 0.125;
                    let xm2 = x + polygon.xm2 as f32 * zoom - 0.125;
                    d.point_rgba(x11,y1,cy1); d.point_rgba(x12,y1,cy1); d.point_rgba(xm2,ym,cym);
                    d.point_rgba(x11,y1,cy1); d.point_rgba(xm2,ym,cym); d.point_rgba(xm1,ym,cym);
                    d.point_rgba(xm1,ym,cym); d.point_rgba(xm2,ym,cym); d.point_rgba(x22,y2,cy2);
                    d.point_rgba(xm1,ym,cym); d.point_rgba(x22,y2,cy2); d.point_rgba(x21,y2,cy2);
                }
            }
        }
    }

    /// Prints a character with given position and color.
    pub fn print_char<ColorT:Blend+Copy>(&self, pixels: &mut SurfacePixels, x: uint, y: uint,
                                         zoom: uint, c: char, color: ColorT) { // XXX #3984
        if !c.is_whitespace() {
            let c = c as uint;
            let glyph = if 32 <= c && c < 126 {c-32} else {0};
            self.print_glyph(pixels, x, y, zoom, glyph, color);
        }
    }

    /// Draws a character with given position and color.
    pub fn draw_char<ColorT:Blend+Copy>(&self, d: &mut glutil::ShadedDrawing, x: f32, y: f32,
                                        zoom: f32, c: char, color: ColorT) { // XXX #3984
        if !c.is_whitespace() {
            let c = c as uint;
            let glyph = if 32 <= c && c < 126 {c-32} else {0};
            self.draw_glyph(d, x, y, zoom, glyph, color);
        }
    }

    /// Prints a string with given position, alignment and color.
    pub fn print_string<ColorT:Blend+Copy>(&self, pixels: &mut SurfacePixels, x: uint, y: uint,
                                           zoom: uint, align: Alignment, s: &str,
                                           color: ColorT) { // XXX #3984
        let mut x = match align {
            LeftAligned  => x,
            Centered     => x - s.char_len() * (NCOLUMNS * zoom) / 2,
            RightAligned => x - s.char_len() * (NCOLUMNS * zoom),
        };
        for s.iter().advance |c| {
            let nextx = x + NCOLUMNS * zoom;
            if nextx >= pixels.width { break; }
            self.print_char(pixels, x, y, zoom, c, copy color);
            x = nextx;
        }
    }

    /// Draws a string with given position, alignment and color.
    pub fn draw_string<ColorT:Blend+Copy>(&self, d: &mut glutil::ShadedDrawing, x: f32, y: f32,
                                          zoom: f32, align: Alignment, s: &str,
                                          color: ColorT) { // XXX #3984
        let mut x = match align {
            LeftAligned  => x,
            Centered     => x - s.char_len() as f32 * (NCOLUMNS as f32 * zoom) / 2.0,
            RightAligned => x - s.char_len() as f32 * (NCOLUMNS as f32 * zoom),
        };
        for s.iter().advance |c| {
            self.draw_char(d, x, y, zoom, c, copy color);
            x += NCOLUMNS as f32 * zoom;
        }
    }
}

