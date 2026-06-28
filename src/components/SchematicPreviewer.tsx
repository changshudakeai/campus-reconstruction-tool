import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { useEffect, useRef } from "react";
import * as THREE from "three";
import type { PreviewCameraView, SchematicModel } from "../domain/schematicModel";
import type { BlockInspection } from "../services/schematicEditing";
import { listInspectableBlocks } from "../services/schematicEditing";
import { minecraftBlockIcon, minecraftBlockTint } from "../services/minecraftBlockCatalog";

interface SchematicPreviewerProps {
  model: SchematicModel;
  selectedBlock: BlockInspection | null;
  previewHint: string;
  onInspectBlock: (block: BlockInspection) => void;
  cameraView: PreviewCameraView;
  showFootprintOverlay: boolean;
  onCapture?: (dataUrl: string) => void;
  captureLabel?: string;
}

const blockColors: Record<string, string> = {
  "minecraft:smooth_stone": "#9da1a3",
  "minecraft:stone_bricks": "#7d8587",
  "minecraft:glass": "#9bd8e6",
  "minecraft:dark_oak_slab": "#4c3323",
  "minecraft:oak_planks": "#c79756",
  "minecraft:quartz_block": "#eee4d0",
  "minecraft:bricks": "#a34f3d",
  "minecraft:mossy_stone_bricks": "#64785f"
  ,"minecraft:dark_oak_door": "#382417"
  ,"minecraft:polished_andesite": "#777b7c"
  ,"minecraft:oxidized_copper": "#4f8f82"
};

export function SchematicPreviewer({
  model,
  selectedBlock,
  previewHint,
  onInspectBlock,
  cameraView,
  showFootprintOverlay,
  onCapture,
  captureLabel
}: SchematicPreviewerProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const inspectRef = useRef(onInspectBlock);
  const selectedBlockRef = useRef(selectedBlock);
  const selectedOutlineRef = useRef<THREE.LineSegments | null>(null);
  const renderPreviewRef = useRef<(() => void) | null>(null);
  const applyCameraViewRef = useRef<((view: PreviewCameraView) => void) | null>(null);
  const captureRef = useRef<(() => string) | null>(null);
  const footprintOverlayRef = useRef<THREE.Group | null>(null);

  useEffect(() => {
    inspectRef.current = onInspectBlock;
  }, [onInspectBlock]);

  useEffect(() => {
    selectedBlockRef.current = selectedBlock;
  }, [selectedBlock]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return undefined;
    const container = host;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color("#f6f3eb");

    const initialWidth = Math.max(container.clientWidth, 1);
    const initialHeight = Math.max(container.clientHeight, 1);
    const camera = new THREE.PerspectiveCamera(42, initialWidth / initialHeight, 0.1, 2000);
    camera.position.set(model.width * 0.8, model.height * 1.7, model.length * 1.15);

    const renderer = new THREE.WebGLRenderer({
      antialias: true,
      powerPreference: "low-power",
      preserveDrawingBuffer: true
    });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.setSize(initialWidth, initialHeight);
    container.appendChild(renderer.domElement);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.target.set(model.width / 2, model.height / 3, model.length / 2);
    controls.enableDamping = false;
    controls.update();

    scene.add(new THREE.AmbientLight("#fff6e8", 1.25));
    const sun = new THREE.DirectionalLight("#ffffff", 2.5);
    sun.position.set(model.width, model.height * 2, model.length);
    scene.add(sun);

    const grid = new THREE.GridHelper(Math.max(model.width, model.length) + 12, 12, "#242c28", "#d4caba");
    grid.position.set(model.width / 2, -0.02, model.length / 2);
    scene.add(grid);

    const footprintOverlay = new THREE.Group();
    const overlayMaterial = new THREE.LineBasicMaterial({ color: "#e05d3f" });
    for (const component of model.metadata.generationReport?.footprintOverlay ?? []) {
      addOverlayRing(footprintOverlay, component.exterior, overlayMaterial);
      for (const ring of component.interiorRings) addOverlayRing(footprintOverlay, ring, overlayMaterial);
    }
    footprintOverlay.visible = showFootprintOverlay;
    scene.add(footprintOverlay);
    footprintOverlayRef.current = footprintOverlay;

    const blocks = listInspectableBlocks(model);
    const blocksByPalette = groupBlocksByPalette(blocks);
    const cube = new THREE.BoxGeometry(1, 1, 1);
    const inspectableMeshes: THREE.InstancedMesh[] = [];

    blocksByPalette.forEach((group, paletteIndex) => {
      if (!group?.length) return;

      const blockName = model.palette[paletteIndex];
      const material = new THREE.MeshStandardMaterial({
        color: minecraftBlockTint(blockName) ?? "#ffffff",
        roughness: 0.86,
        metalness: 0.01,
        transparent: /glass|water|ice/.test(blockName),
        opacity: /glass|water|ice/.test(blockName) ? 0.7 : 1,
        alphaTest: /leaves|sapling|flower|grass|vine|fern/.test(blockName) ? 0.25 : 0
      });
      const texture = new THREE.TextureLoader().load(
        minecraftBlockIcon(blockName),
        () => renderPreview(),
        undefined,
        () => {
          material.map = null;
          material.color.set(blockColors[blockName] ?? "#b9b0a1");
          material.needsUpdate = true;
          renderPreview();
        }
      );
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.magFilter = THREE.NearestFilter;
      texture.minFilter = THREE.NearestFilter;
      material.map = texture;
      const mesh = new THREE.InstancedMesh(cube, material, group.length);
      const matrix = new THREE.Matrix4();

      group.forEach((block, instanceIndex) => {
        matrix.makeTranslation(block.x + 0.5, block.y + 0.5, block.z + 0.5);
        mesh.setMatrixAt(instanceIndex, matrix);
      });

      mesh.instanceMatrix.needsUpdate = true;
      mesh.userData.blocks = group;
      scene.add(mesh);
      inspectableMeshes.push(mesh);
    });

    const selectedOutline = new THREE.LineSegments(
      new THREE.EdgesGeometry(new THREE.BoxGeometry(1.08, 1.08, 1.08)),
      new THREE.LineBasicMaterial({ color: "#d2a43c", linewidth: 2 })
    );
    selectedOutline.visible = false;
    scene.add(selectedOutline);
    selectedOutlineRef.current = selectedOutline;

    const raycaster = new THREE.Raycaster();
    const pointer = new THREE.Vector2();

    function inspectAtPointer(event: PointerEvent) {
      const rect = renderer.domElement.getBoundingClientRect();
      pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
      pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(pointer, camera);

      const hit = raycaster.intersectObjects(inspectableMeshes, false)[0];
      if (!hit || hit.instanceId === undefined) return;

      const blocks = hit.object.userData.blocks as BlockInspection[];
      const block = blocks[hit.instanceId];
      if (block) inspectRef.current(block);
    }

    function resize() {
      const width = Math.max(container.clientWidth, 1);
      const height = Math.max(container.clientHeight, 1);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
      renderer.setSize(width, height);
      renderPreview();
    }

    function renderPreview() {
      const selected = selectedBlockRef.current;
      selectedOutline.visible = Boolean(selected);
      if (selected) {
        selectedOutline.position.set(selected.x + 0.5, selected.y + 0.5, selected.z + 0.5);
      }
      renderer.render(scene, camera);
    }
    renderPreviewRef.current = renderPreview;

    function applyCameraView(view: PreviewCameraView) {
      const center = new THREE.Vector3(model.width / 2, Math.max(1, model.height / 3), model.length / 2);
      const distance = Math.max(model.width, model.height, model.length) * 1.8;
      camera.up.set(0, 1, 0);
      if (view === "top") {
        camera.position.set(center.x, distance, center.z);
        camera.up.set(0, 0, -1);
      } else if (view === "front") {
        camera.position.set(center.x, center.y, model.length + distance);
      } else if (view === "side") {
        camera.position.set(model.width + distance, center.y, center.z);
      } else {
        camera.position.set(model.width * 0.8, model.height * 1.7, model.length * 1.15);
      }
      controls.target.copy(center);
      camera.lookAt(center);
      controls.update();
      renderPreview();
    }
    applyCameraViewRef.current = applyCameraView;
    captureRef.current = () => {
      renderPreview();
      return renderer.domElement.toDataURL("image/png");
    };

    renderer.domElement.addEventListener("pointerdown", inspectAtPointer);
    controls.addEventListener("change", renderPreview);
    window.addEventListener("resize", resize);
    applyCameraView(cameraView);

    return () => {
      window.removeEventListener("resize", resize);
      renderer.domElement.removeEventListener("pointerdown", inspectAtPointer);
      controls.removeEventListener("change", renderPreview);
      controls.dispose();
      scene.traverse((object) => {
        const renderable = object as THREE.Mesh;
        if (renderable.geometry instanceof THREE.BufferGeometry) renderable.geometry.dispose();
        const material = renderable.material;
        if (Array.isArray(material)) material.forEach((item) => item.dispose());
        else {
          (material as THREE.MeshStandardMaterial | undefined)?.map?.dispose();
          material?.dispose();
        }
      });
      renderer.dispose();
      selectedOutlineRef.current = null;
      renderPreviewRef.current = null;
      applyCameraViewRef.current = null;
      captureRef.current = null;
      footprintOverlayRef.current = null;
      container.removeChild(renderer.domElement);
    };
  }, [model]);

  useEffect(() => {
    if (hostRef.current) hostRef.current.dataset.cameraView = cameraView;
    applyCameraViewRef.current?.(cameraView);
  }, [cameraView]);

  useEffect(() => {
    if (hostRef.current) hostRef.current.dataset.footprintOverlay = String(showFootprintOverlay);
    if (footprintOverlayRef.current) footprintOverlayRef.current.visible = showFootprintOverlay;
    renderPreviewRef.current?.();
  }, [showFootprintOverlay]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    host.dataset.selectedBlock = selectedBlock
      ? `${selectedBlock.block} @ ${selectedBlock.x},${selectedBlock.y},${selectedBlock.z}`
      : "none";
    const outline = selectedOutlineRef.current;
    if (outline) {
      outline.visible = Boolean(selectedBlock);
      if (selectedBlock) {
        outline.position.set(
          selectedBlock.x + 0.5,
          selectedBlock.y + 0.5,
          selectedBlock.z + 0.5
        );
      }
    }
    renderPreviewRef.current?.();
  }, [selectedBlock]);

  return (
    <div className="previewer-shell">
      <div className="previewer-canvas" ref={hostRef} aria-label="Three.js schematic previewer" />
      <div className="previewer-hint">
        <span>{previewHint}</span>
        {onCapture ? <button
          className="review-button preview-capture-button"
          onClick={() => {
            const dataUrl = captureRef.current?.();
            if (dataUrl) onCapture(dataUrl);
          }}
        >
          {captureLabel}
        </button> : null}
      </div>
    </div>
  );
}

function addOverlayRing(
  group: THREE.Group,
  ring: Array<{ x: number; z: number }>,
  material: THREE.LineBasicMaterial
) {
  if (ring.length < 2) return;
  const points = ring.map(({ x, z }) => new THREE.Vector3(x, 0.08, z));
  if (!points[0].equals(points[points.length - 1])) points.push(points[0].clone());
  group.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(points), material));
}

function groupBlocksByPalette(blocks: BlockInspection[]): BlockInspection[][] {
  const groups: BlockInspection[][] = [];
  for (const block of blocks) {
    groups[block.paletteIndex] ??= [];
    groups[block.paletteIndex].push(block);
  }
  return groups;
}
