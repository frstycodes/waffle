// shadcn/ui's InputOTP: one digit of a verification code.
#![allow(unused_imports)]
use gpui_kit::*;

use super::*;
use crate::{icons::Glyph, illustration::Illustration, palette};

// One cell of a one-time code: 44x52, the digit in mono bold 20, the focus edge and ring on the cell typed next.
// Keywords: inputotp, shadcn
pub struct InputOTP;

impl InputOTP {
    // One cell of a one-time code: 44x52, the digit in mono bold 20, the focus edge and ring on the cell typed next.
    pub fn slot(digit: Option<char>, focused: bool, cx: &App) -> Div {
        super::code_cell(digit, focused, cx)
    }
}

