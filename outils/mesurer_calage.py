"""Mesure du calage entre la capture du jeu et le rendu de l'outil : similitude (échelle, rotation, décalage)
estimée sur des points d'intérêt appariés. Échelle 1 = même champ de vision."""
import sys
import cv2
import numpy as np

for nom in sys.argv[1:]:
    img = cv2.imread(nom)
    h = img.shape[0]
    jeu = img[h - 540:h, 0:960]
    outil = img[h - 540:h, 964:1924]
    res = []
    for prep in ("clahe", "bords"):
        def pre(x):
            g = cv2.cvtColor(x, cv2.COLOR_BGR2GRAY)
            if prep == "clahe":
                return cv2.createCLAHE(3.0, (8, 8)).apply(g)
            g = cv2.GaussianBlur(g, (0, 0), 1.2)
            sx, sy = cv2.Sobel(g, cv2.CV_32F, 1, 0), cv2.Sobel(g, cv2.CV_32F, 0, 1)
            m = cv2.magnitude(sx, sy)
            return cv2.normalize(m, None, 0, 255, cv2.NORM_MINMAX).astype(np.uint8)
        a, b = pre(jeu), pre(outil)
        # masque : pas la boîte de chat, ni l'arme, ni la barre de vie (bas de l'image du jeu)
        masque = np.full(a.shape, 255, np.uint8)
        masque[380:540, 0:380] = 0
        masque[330:540, 690:960] = 0
        sift = cv2.SIFT_create(4000)
        ka, da = sift.detectAndCompute(a, masque)
        kb, db = sift.detectAndCompute(b, None)
        m = cv2.BFMatcher().knnMatch(da, db, k=2)
        bons = [x for x, y in m if x.distance < 0.75 * y.distance]
        if len(bons) < 12:
            res.append((prep, len(bons), None))
            continue
        pa = np.float32([ka[x.queryIdx].pt for x in bons])
        pb = np.float32([kb[x.trainIdx].pt for x in bons])
        M, inl = cv2.estimateAffinePartial2D(pa, pb, method=cv2.RANSAC, ransacReprojThreshold=3.0)
        n = int(inl.sum())
        ech = float(np.hypot(M[0, 0], M[1, 0]))
        rot = float(np.degrees(np.arctan2(M[1, 0], M[0, 0])))
        # décalage au centre de l'image
        c = M @ np.array([480.0, 270.0, 1.0])
        ecarts = np.linalg.norm((pa[inl.ravel() == 1] @ M[:, :2].T + M[:, 2]) - pb[inl.ravel() == 1], axis=1)
        # écart brut, sans aucune transformation, sur les points retenus
        brut = np.linalg.norm(pa[inl.ravel() == 1] - pb[inl.ravel() == 1], axis=1)
        res.append((prep, len(bons), dict(retenus=n, echelle=round(ech, 4), rotation=round(rot, 3),
                                         decalage_centre=(round(float(c[0] - 480), 2), round(float(c[1] - 270), 2)),
                                         ecart_brut_median=round(float(np.median(brut)), 2), ecart_brut_max=round(float(brut.max()), 2),
                                         etendue=(int(pa[inl.ravel() == 1][:, 0].min()), int(pa[inl.ravel() == 1][:, 0].max()),
                                                  int(pa[inl.ravel() == 1][:, 1].min()), int(pa[inl.ravel() == 1][:, 1].max())))))
    print(nom)
    for r in res:
        print("  ", r)
