//! Scheduler: orden de ejecución de los sistemas.
//!
//! El scheduler ejecuta los sistemas **en el orden en que se registraron**.
//! Nada queda oculto: cada sistema declara su acceso ([`Access`]) y el
//! scheduler lo usa para:
//!
//! 1. Validar que no existan dependencias no declaradas.
//! 2. Calcular *etapas* ([`Stage`]): conjuntos de sistemas sin conflicto que,
//!    en el futuro, podrán ejecutarse en paralelo con préstamos disjuntos.
//! 3. Exponer el plan de ejecución para inspección y pruebas.

mod system;

pub use system::{Access, System, SystemContext};

/// Un grupo de sistemas sin conflictos de acceso entre sí.
#[derive(Debug, Clone, Default)]
pub struct Stage {
    /// Índices en `Scheduler::systems`.
    pub systems: Vec<usize>,
}

impl Stage {
    pub fn len(&self) -> usize {
        self.systems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }
}

/// Scheduler secuencial con análisis de etapas.
pub struct Scheduler {
    systems: Vec<Box<dyn System>>,
    staged: Vec<Stage>,
    dirty: bool,
    /// Ejecuciones acumuladas (sistemas totales).
    executions: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            staged: Vec::new(),
            dirty: false,
            executions: 0,
        }
    }

    /// Registra un sistema al final del schedule.
    pub fn add_system(&mut self, system: impl System + 'static) {
        self.systems.push(Box::new(system));
        self.dirty = true;
    }

    /// Sistemas registrados (en orden).
    pub fn systems(&self) -> &[Box<dyn System>] {
        &self.systems
    }

    /// Recalcula el plan de etapas si hace falta.
    pub fn plan(&mut self) -> &[Stage] {
        if self.dirty || self.staged.is_empty() {
            self.staged = compute_stages(&self.systems);
            self.dirty = false;
        }
        &self.staged
    }

    /// Ejecuta todos los sistemas **en el orden en que se registraron**.
    ///
    /// El orden de registro es el orden que el autor del universo eligió; las
    /// etapas ([`plan`]) se calculan solo para inspección y para habilitar, en
    /// el futuro, ejecución paralela de etapas sin cambiar la semántica.
    pub fn run(&mut self, ctx: &mut SystemContext<'_>) {
        for i in 0..self.systems.len() {
            self.systems[i].run(ctx);
            self.executions += 1;
        }
        ctx.stats.systems_run = self.executions;
    }

    /// Total de ejecuciones de sistema acumuladas.
    pub fn executions(&self) -> u64 {
        self.executions
    }
}

/// Calcula etapas máximas sin conflicto respetando el orden de registro.
///
/// Algoritmo goloso: cada sistema se asigna a la **primera** etapa donde no
/// compite con ningún miembro actual. Como los conflictos imponen un orden
/// parcial, esta asignación preserva la semántica del orden original.
pub fn compute_stages(systems: &[Box<dyn System>]) -> Vec<Stage> {
    let mut stages: Vec<Stage> = Vec::new();
    for (i, sys) in systems.iter().enumerate() {
        let acc = sys.access();
        let mut placed = false;
        for stage in stages.iter_mut() {
            let conflicts = stage
                .systems
                .iter()
                .any(|&j| systems[j].access().conflicts_with(&acc));
            if !conflicts {
                stage.systems.push(i);
                placed = true;
                break;
            }
        }
        if !placed {
            stages.push(Stage {
                systems: vec![i],
            });
        }
    }
    stages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{Component, ComponentId};

    struct A;
    impl Component for A {
        const ID: ComponentId = ComponentId(50);
    }

    #[allow(dead_code)]
    struct B;
    impl Component for B {
        const ID: ComponentId = ComponentId(51);
    }

    struct SysA;
    impl System for SysA {
        fn name(&self) -> &'static str {
            "sysA"
        }
        fn access(&self) -> Access {
            Access::default().writes::<A>()
        }
        fn run(&mut self, _ctx: &mut SystemContext<'_>) {}
    }

    struct SysB;
    impl System for SysB {
        fn name(&self) -> &'static str {
            "sysB"
        }
        fn access(&self) -> Access {
            Access::default().reads::<A>()
        }
        fn run(&mut self, _ctx: &mut SystemContext<'_>) {}
    }

    struct SysC;
    impl System for SysC {
        fn name(&self) -> &'static str {
            "sysC"
        }
        fn access(&self) -> Access {
            Access::default().writes::<B>()
        }
        fn run(&mut self, _ctx: &mut SystemContext<'_>) {}
    }

    #[test]
    fn conflictos_detectados() {
        let w = Access::default().writes::<A>();
        let r = Access::default().reads::<A>();
        assert!(w.conflicts_with(&r));
        assert!(!r.conflicts_with(&Access::default().reads::<A>()));
        assert!(!Access::default().reads::<A>().conflicts_with(&Access::default().reads::<B>()));
    }

    #[test]
    fn etapas_respetan_dependencias() {
        // SysA escribe A; SysB lee A => conflicto (orden relativo forzado).
        // SysC escribe B: no compite ni con A ni con B, puede adelantarse.
        let systems: Vec<Box<dyn System>> =
            vec![Box::new(SysA), Box::new(SysB), Box::new(SysC)];
        let stages = compute_stages(&systems);

        // Invariante central: todo sistema que *conflicta* con otro anterior
        // queda en una etapa posterior (o igual), nunca antes.
        for i in 0..systems.len() {
            for j in (i + 1)..systems.len() {
                if systems[i].access().conflicts_with(&systems[j].access()) {
                    let si = stages.iter().position(|s| s.systems.contains(&i)).unwrap();
                    let sj = stages.iter().position(|s| s.systems.contains(&j)).unwrap();
                    assert!(si <= sj, "dependencia {i}->{j} violada: etapa {si} > {sj}");
                }
            }
        }

        // Dentro de una etapa no puede haber conflictos.
        for stage in &stages {
            for (k, &x) in stage.systems.iter().enumerate() {
                for &y in &stage.systems[k + 1..] {
                    assert!(
                        !systems[x].access().conflicts_with(&systems[y].access()),
                        "etapa con sistemas en conflicto: {x} y {y}"
                    );
                }
            }
        }
    }
}
