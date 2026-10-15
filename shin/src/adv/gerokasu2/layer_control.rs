//! Retained layer addresses, raw property controls and drawable wait queries.
use super::layers::Gerokasu2Layers;
use crate::layer::LayerProperties;
use anyhow::{Result, bail, ensure};
use enum_map::Enum;
use shin_core::{
    time::{Easing, Ticks},
    vm::{
        command::types::{LayerProperty, LayerbankId, PlaneId},
        gerokasu2::Gerokasu2RuntimeCommand as Cmd,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyTarget {
    Root,
    Screen,
    Page,
    Message,
    Plane(PlaneId),
    Bank { plane: PlaneId, bank: LayerbankId },
}
pub trait LayerControlServices {
    fn begin_property_change(&mut self) -> Result<()> {
        Ok(())
    }
    /// An owner without the outer message/screen root must preserve an explicit
    /// continuation for root commands, rather than applying them to Screen.
    fn supports_root(&self) -> bool {
        false
    }
    /// Validate every target before mutation; an error must not apply a partial
    /// control. Visiting a property facade does not force lazy initialization.
    fn visit_properties(
        &mut self,
        targets: &[PropertyTarget],
        action: &mut dyn FnMut(&mut LayerProperties),
    ) -> Result<()>;
    fn bank_wiping(&mut self, plane: PlaneId, bank: LayerbankId) -> Result<bool>;
}
fn capture(layers: &mut Gerokasu2Layers, id: i32) -> Result<Vec<PropertyTarget>> {
    let plane = layers.current_plane();
    Ok(match id {
        -1 => vec![PropertyTarget::Root],
        -2 => vec![PropertyTarget::Screen],
        -3 => vec![PropertyTarget::Page],
        -4 => vec![PropertyTarget::Plane(plane)],
        -5 => vec![PropertyTarget::Message],
        _ => layers
            .targets(id)?
            .iter()
            .map(|t| PropertyTarget::Bank {
                plane,
                bank: t.layerbank,
            })
            .collect(),
    })
}
fn property(id: i32) -> Result<LayerProperty> {
    ensure!((0..90).contains(&id), "Invalid Gerokasu2 property {id}");
    Ok(LayerProperty::from_usize(id as usize))
}
#[derive(Debug)]
pub struct LayerControlPlan {
    targets: Vec<PropertyTarget>,
    property: LayerProperty,
    value: f32,
    duration: Ticks,
    flags: u32,
    easing: Easing,
    ignore_wait: bool,
}
impl LayerControlPlan {
    /// Preparation mutates only a caller-owned integer state copy. Commit that
    /// copy after the drawable action succeeds, so errors are retryable.
    pub fn prepare(layers: &mut Gerokasu2Layers, command: &Cmd) -> Result<Self> {
        let Cmd::LayerCtrl {
            layer_id,
            property_id,
            params,
        } = command
        else {
            bail!("Expected LAYERCTRL")
        };
        let property = property(*property_id)?;
        let flags = params.2 as u32;
        let easing = match flags & 63 {
            0 => Easing::Linear,
            1 => Easing::SineIn,
            2 => Easing::SineOut,
            3 => Easing::SineInOut,
            4 => Easing::Jump,
            5 => Easing::Power(params.3),
            n => bail!("Invalid Gerokasu2 easing {n}"),
        };
        let targets = capture(layers, *layer_id)?;
        if *layer_id != -5 {
            layers.control(*layer_id, *property_id, params.0, params.2)?;
        }
        Ok(Self {
            targets,
            property,
            value: params.0 as f32,
            duration: Ticks::from_f32(params.1 as f32),
            flags: flags & 0xffff,
            easing,
            ignore_wait: flags & 0x10000 != 0,
        })
    }
    pub fn needs_root(&self) -> bool {
        self.targets.contains(&PropertyTarget::Root)
            || self.targets.contains(&PropertyTarget::Message)
    }
    pub fn apply(&self, services: &mut dyn LayerControlServices) -> Result<()> {
        services.begin_property_change()?;
        services.visit_properties(&self.targets, &mut |props| {
            props.property_tweener_mut(self.property).gerokasu2_control(
                self.value,
                self.duration,
                self.flags,
                self.easing,
                self.ignore_wait,
            )
        })
    }
}
pub fn initialize(
    layers: &mut Gerokasu2Layers,
    id: i32,
    services: &mut dyn LayerControlServices,
) -> Result<()> {
    // Native INIT does not recognize Message -5; its unchecked allocator
    // lookup can alias bank47 on plane0. Reject before mutating Message.
    ensure!(
        id != -5,
        "Ger LAYERINIT -5 is an unsupported native bank address"
    );
    let targets = capture(layers, id)?;
    services.begin_property_change()?;
    services.visit_properties(&targets, &mut LayerProperties::init)?;
    layers.initialize(id)?;
    Ok(())
}
#[derive(Debug)]
pub struct LayerWait {
    targets: Vec<PropertyTarget>,
    properties: Vec<LayerProperty>,
}
impl LayerWait {
    pub fn capture(layers: &mut Gerokasu2Layers, id: i32, properties: &[i32]) -> Result<Self> {
        let properties = properties
            .iter()
            .copied()
            .map(property)
            .collect::<Result<_>>()?;
        Ok(Self {
            targets: capture(layers, id)?,
            properties,
        })
    }
    pub fn needs_root(&self) -> bool {
        self.targets.contains(&PropertyTarget::Root)
    }
    pub fn poll(&self, services: &mut dyn LayerControlServices, skip: bool) -> Result<bool> {
        let mut waiting = false;
        services.visit_properties(&self.targets, &mut |props| {
            if waiting {
                return;
            }
            for &property in &self.properties {
                let mut tween = props.property_tweener_mut(property);
                if skip {
                    tween.fast_forward_unless_prohibited();
                }
                if tween.is_waiting() {
                    waiting = true;
                    break;
                }
            }
        })?;
        Ok(!waiting)
    }
}
#[derive(Debug)]
pub struct TransitionWait {
    targets: Vec<PropertyTarget>,
}
impl TransitionWait {
    pub fn capture(layers: &mut Gerokasu2Layers, id: i32) -> Result<Self> {
        Ok(Self {
            targets: capture(layers, id)?,
        })
    }
    pub fn poll(&self, services: &mut dyn LayerControlServices) -> Result<bool> {
        for &target in &self.targets {
            if let PropertyTarget::Bank { plane, bank } = target {
                if services.bank_wiping(plane, bank)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
