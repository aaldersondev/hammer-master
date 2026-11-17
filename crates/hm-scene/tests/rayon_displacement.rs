//! Un rayon touche le relief d'un displacement, pas la brosse de base dessous : dalle de 256 × 256 dont le dessus
//! est un displacement de puissance 2 relevé de 64 partout.
use glam::Vec3;
use hm_scene::{Scene, SceneOptions};

const VMF: &str = r#"world
{
	"id" "1"
	"classname" "worldspawn"
	solid
	{
		"id" "10"
		side
		{
			"id" "2"
			"plane" "(0 256 0) (256 256 0) (256 0 0)"
			"material" "DEV/DEV_MEASUREGENERIC01"
			"uaxis" "[1 0 0 0] 0.25"
			"vaxis" "[0 -1 0 0] 0.25"
			"rotation" "0"
			"lightmapscale" "16"
			dispinfo
			{
				"power" "2"
				"startposition" "[0 0 0]"
				"flags" "0"
				"elevation" "0"
				"subdiv" "0"
				normals
				{
					"row0" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
					"row1" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
					"row2" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
					"row3" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
					"row4" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
				}
				distances
				{
					"row0" "64 64 64 64 64"
					"row1" "64 64 64 64 64"
					"row2" "64 64 64 64 64"
					"row3" "64 64 64 64 64"
					"row4" "64 64 64 64 64"
				}
				offsets
				{
					"row0" "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"
					"row1" "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"
					"row2" "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"
					"row3" "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"
					"row4" "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"
				}
				offset_normals
				{
					"row0" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
					"row1" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
					"row2" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
					"row3" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
					"row4" "0 0 1 0 0 1 0 0 1 0 0 1 0 0 1"
				}
				alphas
				{
					"row0" "0 0 0 0 0"
					"row1" "0 0 0 0 0"
					"row2" "0 0 0 0 0"
					"row3" "0 0 0 0 0"
					"row4" "0 0 0 0 0"
				}
				triangle_tags
				{
					"row0" "9 9 9 9 9 9 9 9"
					"row1" "9 9 9 9 9 9 9 9"
					"row2" "9 9 9 9 9 9 9 9"
					"row3" "9 9 9 9 9 9 9 9"
				}
				allowed_verts
				{
					"10" "-1 -1 -1 -1 -1 -1 -1 -1 -1 -1"
				}
			}
		}
		side
		{
			"id" "3"
			"plane" "(0 0 -16) (256 0 -16) (256 256 -16)"
			"material" "DEV/DEV_MEASUREGENERIC01"
			"uaxis" "[1 0 0 0] 0.25"
			"vaxis" "[0 -1 0 0] 0.25"
			"rotation" "0"
			"lightmapscale" "16"
		}
		side
		{
			"id" "4"
			"plane" "(0 256 0) (0 0 0) (0 0 -16)"
			"material" "DEV/DEV_MEASUREGENERIC01"
			"uaxis" "[1 0 0 0] 0.25"
			"vaxis" "[0 -1 0 0] 0.25"
			"rotation" "0"
			"lightmapscale" "16"
		}
		side
		{
			"id" "5"
			"plane" "(256 256 -16) (256 0 -16) (256 0 0)"
			"material" "DEV/DEV_MEASUREGENERIC01"
			"uaxis" "[1 0 0 0] 0.25"
			"vaxis" "[0 -1 0 0] 0.25"
			"rotation" "0"
			"lightmapscale" "16"
		}
		side
		{
			"id" "6"
			"plane" "(256 256 0) (0 256 0) (0 256 -16)"
			"material" "DEV/DEV_MEASUREGENERIC01"
			"uaxis" "[1 0 0 0] 0.25"
			"vaxis" "[0 -1 0 0] 0.25"
			"rotation" "0"
			"lightmapscale" "16"
		}
		side
		{
			"id" "7"
			"plane" "(256 0 -16) (0 0 -16) (0 0 0)"
			"material" "DEV/DEV_MEASUREGENERIC01"
			"uaxis" "[1 0 0 0] 0.25"
			"vaxis" "[0 -1 0 0] 0.25"
			"rotation" "0"
			"lightmapscale" "16"
		}
	}
}
"#;

#[test]
fn rayon_touche_le_relief() {
    let vfs = hm_assets::Vfs::new();
    let s = Scene::from_bytes(std::path::Path::new("disp.vmf"), VMF.as_bytes().to_vec(), &vfs, &SceneOptions::default()).unwrap();
    let (dist, obj) = s.rayon(Vec3::new(100.0, 140.0, 500.0), Vec3::NEG_Z).expect("le rayon doit toucher");
    assert!((500.0 - dist - 64.0).abs() < 0.01, "impact à z = {}, attendu 64 (le relief)", 500.0 - dist);
    assert_ne!(obj, 0);
    // à côté de la dalle : rien
    assert!(s.rayon(Vec3::new(400.0, 140.0, 500.0), Vec3::NEG_Z).is_none());
    // par-dessous aussi : le relief est vu des deux côtés, avant la brosse de base
    let (d2, _) = s.rayon(Vec3::new(100.0, 140.0, 30.0), Vec3::Z).expect("par-dessous");
    assert!((30.0 + d2 - 64.0).abs() < 0.01, "impact à z = {}", 30.0 + d2);
}

#[test]
fn seul_le_displacement_est_dessine() {
    // vbsp ne garde d'une brosse à displacement que le displacement : ses cinq autres faces ne sont pas dessinées
    // (vu le 15/11 sur rp_rockford : les côtés en dev/nodraw de ces brosses barraient la vue, absents chez Hammer++)
    let vfs = hm_assets::Vfs::new();
    let s = Scene::from_bytes(std::path::Path::new("disp.vmf"), VMF.as_bytes().to_vec(), &vfs, &SceneOptions::default()).unwrap();
    assert_eq!(s.report.world_triangles, 4 * 4 * 2, "les 32 triangles du displacement de puissance 2, rien d'autre");
}
