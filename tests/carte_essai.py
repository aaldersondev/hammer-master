"""Écrit une petite map d'essai fermée (une salle de 512 × 512 × 256, un point d'apparition, une lampe).

    python tests/carte_essai.py <sortie.vmf>

Sert à essayer en vrai, sur le compilateur et sur un serveur, ce qui ne se prouve pas hors du jeu : l'estampille de
révision ne fait pas de fuite, survit à la compilation et se relit côté serveur.
"""
import sys

MAT = "DEV/DEV_MEASUREGENERIC01B"
ident = [1]


def suivant():
    ident[0] += 1
    return ident[0]


def boite(x0, y0, z0, x1, y1, z1):
    faces = [
        ((x0, y1, z1), (x1, y1, z1), (x1, y0, z1), "[1 0 0 0] 0.25", "[0 -1 0 0] 0.25"),
        ((x0, y0, z0), (x1, y0, z0), (x1, y1, z0), "[1 0 0 0] 0.25", "[0 -1 0 0] 0.25"),
        ((x0, y1, z1), (x0, y0, z1), (x0, y0, z0), "[0 1 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ((x1, y1, z0), (x1, y0, z0), (x1, y0, z1), "[0 1 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ((x1, y1, z1), (x0, y1, z1), (x0, y1, z0), "[1 0 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ((x1, y0, z0), (x0, y0, z0), (x0, y0, z1), "[1 0 0 0] 0.25", "[0 0 -1 0] 0.25"),
    ]
    s = f'\tsolid\n\t{{\n\t\t"id" "{suivant()}"\n'
    for a, b, c, u, v in faces:
        plan = " ".join("(%d %d %d)" % p for p in (a, b, c))
        s += (f'\t\tside\n\t\t{{\n\t\t\t"id" "{suivant()}"\n\t\t\t"plane" "{plan}"\n\t\t\t"material" "{MAT}"\n'
              f'\t\t\t"uaxis" "{u}"\n\t\t\t"vaxis" "{v}"\n\t\t\t"rotation" "0"\n\t\t\t"lightmapscale" "16"\n'
              f'\t\t\t"smoothing_groups" "0"\n\t\t}}\n')
    return s + "\t}\n"


def carte():
    e = 16  # épaisseur des murs
    murs = [
        boite(-256, -256, -e, 256, 256, 0), boite(-256, -256, 256, 256, 256, 256 + e),
        boite(-256 - e, -256, 0, -256, 256, 256), boite(256, -256, 0, 256 + e, 256, 256),
        boite(-256, -256 - e, 0, 256, -256, 256), boite(-256, 256, 0, 256, 256 + e, 256),
    ]
    s = ('versioninfo\n{\n\t"editorversion" "400"\n\t"editorbuild" "8000"\n\t"mapversion" "1"\n\t"formatversion" "100"\n\t"prefab" "0"\n}\n'
         f'world\n{{\n\t"id" "1"\n\t"mapversion" "1"\n\t"classname" "worldspawn"\n\t"skyname" "sky_day01_01"\n' + "".join(murs) + "}\n")
    s += f'entity\n{{\n\t"id" "{suivant()}"\n\t"classname" "info_player_start"\n\t"angles" "0 0 0"\n\t"origin" "0 0 8"\n}}\n'
    s += f'entity\n{{\n\t"id" "{suivant()}"\n\t"classname" "light"\n\t"_light" "255 255 255 300"\n\t"origin" "0 0 200"\n}}\n'
    s += 'cameras\n{\n\t"activecamera" "-1"\n}\n'
    return s


if __name__ == "__main__":
    open(sys.argv[1], "w", encoding="utf8", newline="\r\n").write(carte())
    print("écrit :", sys.argv[1])
