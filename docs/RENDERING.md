# Rendering and PNG

The Studio contains a software map renderer and a PNG encoder. There is no image
crate, no font file and no map tile.

## Projection

The map is equirectangular: longitude maps linearly to x, latitude to y. Each
pixel asks the icosphere for its nearest cell using a latitude/longitude bin
index. At 1024×512 and 10,242 cells, the result is a faithful cell mosaic; at
high zoom, cell boundaries are visible as facets.

Cell centers are used for all vector overlays. Boundary lines are drawn between
cell centers with horizontal wrap handling at the date line.

## Layers

- **Atlas** — biome palette for land, hypsometric ocean, soft plate boundaries,
  rivers.
- **Elevation** — seven-stop hypsometric ramp and hillshade.
- **Plates** — twelve categorical colors, uplift tint, boundary lines colored by
  convergent/divergent/transform type.
- **Biomes** — seventeen fixed colors; water is ocean blue.
- **Temperature** — six-stop ramp from −35 to 38 °C, ocean darkened.
- **Precipitation** — six-stop ramp from 0 to 3,500 mm/yr.
- **Rivers** — muted hypsometric base, rivers weighted by Strahler order, lakes,
  delta points.
- **Political** — desaturated hypsometric base, per-cell territory tint, borders
  between owners and territory edges, cities, capitals and labels.
- **Biodiversity** — six-stop richness ramp.
- **Stress** — quiet, convergent, divergent, transform and volcanic colors.

Coastlines are darkened where a land cell touches water. Hillshade is computed
once per cell from east/north elevation differences, lit from the northwest,
and applied only where requested.

## Political overlay

The renderer takes the ownership snapshot nearest the requested year. Each cell
takes 52% of its civilization color over the desaturated base (30% for the
optional overlay on other layers). Cross-owner edges and territory edges are
drawn as one-pixel dark lines between cell centers. Capitals are white-ringed
squares, other cities are small squares.

Labels use a built-in 5×7 bitmap font. They are clipped to the image and start
position is clamped so a label does not begin past the right edge.

## PNG encoder

- 8-bit RGB, truecolor, no alpha, no interlace.
- One `IHDR`, one `IDAT`, one `IEND`.
- Raw scanlines use filter type 0.
- CRC-32 over chunk type and data.
- zlib container with header `78 01` and Adler-32 trailer.
- DEFLATE: one final fixed-Huffman block with LZ77 matching, 32 KiB window,
  minimum match 3, maximum 258, hash chains bounded to 48 candidates.

The encoder is deterministic: identical pixels produce identical bytes. The
decoder in the test suite supports stored and fixed-Huffman blocks and verifies
the Adler trailer, so CRCs, bit order and pixel filtering are tested, not
assumed.

## Performance

At 1024×512, a layer takes about 123–133 ms and PNG encoding about 34 ms on the
development machine. Nearest-cell lookup dominates. See
[docs/PERFORMANCE.md](PERFORMANCE.md).

## Adding a layer

1. Add a variant to `Layer` and its code/label/legend entries.
2. Fill `layer_colors` for per-cell color.
3. Add any overlay pass in `render`.
4. Add an assertion to the render test that the layer has more than a few unique
   colors and encodes a valid PNG.
5. The Studio reads layer metadata from `/api/world`; no client change is needed
   for a simple layer.

## What is deliberately absent

- No antialiasing of cell edges.
- No contour lines or hachures.
- No map projection other than equirectangular.
- No labels anywhere except civilization seats.
- No external basemap, relief tiles or fonts.
