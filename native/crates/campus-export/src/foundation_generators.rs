use campus_state::{FeatureKind, FoundationGeneratorStyle, MapFeature};

pub(crate) struct FoundationRenderTarget<'a> {
    pub blocks: &'a mut [u16],
    pub width: usize,
    pub height: usize,
    pub length: usize,
    pub palette: &'a [String],
}

pub(crate) struct FoundationFeatureRender<'a> {
    pub feature: &'a MapFeature,
    pub points: &'a [(i32, i32)],
    pub style: Option<&'a FoundationGeneratorStyle>,
    pub palette_index: u16,
    pub road_width_blocks: i32,
}

pub(crate) struct FoundationFeatureGeneratorRegistry;

impl FoundationFeatureGeneratorRegistry {
    pub fn render(
        target: &mut FoundationRenderTarget<'_>,
        render: FoundationFeatureRender<'_>,
    ) -> Result<(), String> {
        let generator = render
            .style
            .map(|style| style.generator.as_str())
            .unwrap_or("core:solid-fill/v1");
        match generator {
            "arnis:road/v1" if render.feature.kind == FeatureKind::Road => {
                RoadGenerator.render(target, &render)
            }
            "arnis:water/v1" if render.feature.kind == FeatureKind::Water => {
                OutlinedAreaGenerator.render(target, &render)
            }
            "arnis:sports/v1" if render.feature.kind == FeatureKind::Sports => {
                OutlinedAreaGenerator.render(target, &render)
            }
            "arnis:vegetation/v1" if render.feature.kind == FeatureKind::Vegetation => {
                VegetationGenerator.render(target, &render)
            }
            "core:solid-fill/v1" => SolidFillGenerator.render(target, &render),
            registered => Err(format!(
                "Foundation Feature Generator {registered} 不支持 {:?}",
                render.feature.kind
            )),
        }
    }
}

trait FoundationFeatureGeneratorAdapter {
    fn render(
        &self,
        target: &mut FoundationRenderTarget<'_>,
        render: &FoundationFeatureRender<'_>,
    ) -> Result<(), String>;
}

struct SolidFillGenerator;

impl FoundationFeatureGeneratorAdapter for SolidFillGenerator {
    fn render(
        &self,
        target: &mut FoundationRenderTarget<'_>,
        render: &FoundationFeatureRender<'_>,
    ) -> Result<(), String> {
        if render.feature.kind == FeatureKind::Road {
            super::draw_polyline(
                target.blocks,
                target.width,
                target.length,
                1,
                render.points,
                (render.road_width_blocks.max(1) / 2).max(1),
                render.palette_index,
            );
        } else if render.points.len() >= 3 {
            super::fill_polygon(
                target.blocks,
                target.width,
                target.length,
                1,
                render.points,
                render.palette_index,
            );
        }
        Ok(())
    }
}

struct RoadGenerator;

impl FoundationFeatureGeneratorAdapter for RoadGenerator {
    fn render(
        &self,
        target: &mut FoundationRenderTarget<'_>,
        render: &FoundationFeatureRender<'_>,
    ) -> Result<(), String> {
        let road_width = render.road_width_blocks.max(1);
        let edge_index =
            super::secondary_palette_index(render.style, target.palette, render.palette_index, 1);
        super::draw_polyline(
            target.blocks,
            target.width,
            target.length,
            1,
            render.points,
            ((road_width + 2) / 2).max(1),
            edge_index,
        );
        super::draw_polyline(
            target.blocks,
            target.width,
            target.length,
            1,
            render.points,
            (road_width / 2).max(1),
            render.palette_index,
        );
        Ok(())
    }
}

struct OutlinedAreaGenerator;

impl FoundationFeatureGeneratorAdapter for OutlinedAreaGenerator {
    fn render(
        &self,
        target: &mut FoundationRenderTarget<'_>,
        render: &FoundationFeatureRender<'_>,
    ) -> Result<(), String> {
        if render.points.len() < 3 {
            return Ok(());
        }
        super::fill_polygon(
            target.blocks,
            target.width,
            target.length,
            1,
            render.points,
            render.palette_index,
        );
        let border_index =
            super::secondary_palette_index(render.style, target.palette, render.palette_index, 1);
        super::draw_polygon_outline(
            target.blocks,
            target.width,
            target.length,
            1,
            render.points,
            border_index,
        );
        Ok(())
    }
}

struct VegetationGenerator;

impl FoundationFeatureGeneratorAdapter for VegetationGenerator {
    fn render(
        &self,
        target: &mut FoundationRenderTarget<'_>,
        render: &FoundationFeatureRender<'_>,
    ) -> Result<(), String> {
        if render.points.len() < 3 {
            return Ok(());
        }
        super::fill_polygon(
            target.blocks,
            target.width,
            target.length,
            1,
            render.points,
            render.palette_index,
        );
        let log_index =
            super::secondary_palette_index(render.style, target.palette, render.palette_index, 1);
        let leaves_index =
            super::secondary_palette_index(render.style, target.palette, render.palette_index, 2);
        let density = render
            .style
            .and_then(|style| style.density)
            .unwrap_or(0.035);
        let seed = render.style.and_then(|style| style.seed).unwrap_or(1);
        super::draw_vegetation_trees(
            target.blocks,
            target.width,
            target.height,
            target.length,
            render.points,
            log_index,
            leaves_index,
            density,
            seed,
        );
        Ok(())
    }
}
