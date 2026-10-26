//! Máquina del cierre: qué hacer cuando hay cambios sin guardar.
//! Un solo valor: no puede haber dos pedidos de cierre a la vez.

use crate::workspace::NoteId;

/// Qué se estaba cerrando cuando apareció el modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Tab,
    Window,
    Restart,
}

/// Lo que se ejecuta en el frame siguiente.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    CloseWindow,
    Relaunch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Closing {
    #[default]
    Idle,
    /// El modal espera respuesta por `id`.
    Asking { id: NoteId, intent: Intent },
    /// Cerrar la ventana en el próximo frame.
    Window,
    /// Relanzar el binario nuevo.
    Relaunching,
}

impl Closing {
    pub fn ask(&mut self, id: NoteId, intent: Intent) {
        *self = Self::Asking { id, intent };
    }

    pub fn question(&self) -> Option<(NoteId, Intent)> {
        match *self {
            Self::Asking { id, intent } => Some((id, intent)),
            _ => None,
        }
    }

    pub fn is_asking(&self) -> bool {
        matches!(self, Self::Asking { .. })
    }

    /// Hay un cierre esperando el frame siguiente.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Window | Self::Relaunching)
    }

    /// El modal ya tiene respuesta: deja de estar abierto.
    pub fn resolved(&mut self) -> Option<(NoteId, Intent)> {
        let question = self.question();
        if question.is_some() {
            *self = Self::Idle;
        }
        question
    }

    pub fn cancel(&mut self) {
        *self = Self::Idle;
    }

    pub fn finish(&mut self, action: Action) {
        *self = match action {
            Action::CloseWindow => Self::Window,
            Action::Relaunch => Self::Relaunching,
        };
    }

    /// Consume el cierre pendiente: se ejecuta una sola vez.
    pub fn take(&mut self) -> Option<Action> {
        let action = match self {
            Self::Window => Action::CloseWindow,
            Self::Relaunching => Action::Relaunch,
            _ => return None,
        };
        *self = Self::Idle;
        Some(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> NoteId {
        NoteId::for_test(n)
    }

    #[test]
    fn pregunta_por_la_nota_y_el_motivo() {
        let mut closing = Closing::Idle;
        closing.ask(id(7), Intent::Window);
        assert_eq!(closing.question(), Some((id(7), Intent::Window)));
        assert!(closing.is_asking());
    }

    #[test]
    fn resolver_vacia_la_pregunta() {
        let mut closing = Closing::Idle;
        closing.ask(id(1), Intent::Tab);
        assert_eq!(closing.resolved(), Some((id(1), Intent::Tab)));
        assert_eq!(closing.question(), None);
    }

    #[test]
    fn cancelar_vuelve_a_idle() {
        let mut closing = Closing::Idle;
        closing.ask(id(1), Intent::Restart);
        closing.cancel();
        assert_eq!(closing, Closing::Idle);
    }

    #[test]
    fn un_pedido_nuevo_reemplaza_el_anterior() {
        let mut closing = Closing::Idle;
        closing.ask(id(1), Intent::Tab);
        closing.ask(id(2), Intent::Restart);
        assert_eq!(closing.question(), Some((id(2), Intent::Restart)));
    }

    #[test]
    fn cerrar_se_ejecuta_una_sola_vez() {
        let mut closing = Closing::Idle;
        closing.finish(Action::CloseWindow);
        assert!(closing.is_pending());
        assert_eq!(closing.take(), Some(Action::CloseWindow));
        assert_eq!(closing.take(), None);
        assert_eq!(closing, Closing::Idle);
    }

    #[test]
    fn relanzar_y_despues_cerrar() {
        let mut closing = Closing::Idle;
        closing.finish(Action::Relaunch);
        assert_eq!(closing.take(), Some(Action::Relaunch));
        closing.finish(Action::CloseWindow);
        assert_eq!(closing.take(), Some(Action::CloseWindow));
    }
}
