"""Render a captured pyte grid, never a fabricated native Terminal screenshot."""
from PIL import Image, ImageDraw, ImageFont

font = ImageFont.truetype('/System/Library/Fonts/Menlo.ttc', 18)
cjk = ImageFont.truetype('/System/Library/Fonts/Supplemental/Songti.ttc', 19)
colors = {'default':'#d9dcdf','black':'#202226','red':'#e57a7a','green':'#96c995','brown':'#d4975e','blue':'#84a7d9','magenta':'#c098cf','cyan':'#87c9ca','white':'#d9dcdf','brightblack':'#888e98'}

def shot(screen, name, out):
    image = Image.new('RGB', (screen.columns*11+32, screen.lines*26+28), '#17191c')
    draw = ImageDraw.Draw(image)
    for row in range(screen.lines):
        for col in range(screen.columns):
            cell = screen.buffer[row][col]
            if cell.bg != 'default':
                background=colors.get(cell.bg, '#'+cell.bg if len(cell.bg)==6 else '#17191c')
                draw.rectangle((16+col*11,12+row*26,16+(col+1)*11,12+(row+1)*26),fill=background)
            if not cell.data.strip(): continue
            color = colors.get(cell.fg, '#'+cell.fg if len(cell.fg)==6 else '#d9dcdf')
            if cell.data in ('█', '▀'):
                block_height = 26 if cell.data == '█' else 13
                draw.rectangle((16+col*11,12+row*26,16+(col+1)*11-1,12+row*26+block_height-1),fill=color)
                continue
            draw.text((16+col*11, 12+row*26), cell.data, fill=color, font=cjk if any(ord(c)>256 for c in cell.data) else font)
    image.save(out / (name+'.png'))
    (out / (name+'.txt')).write_text('\n'.join(''.join(screen.buffer[row][col].data for col in range(screen.columns)) for row in range(screen.lines))+'\n')
