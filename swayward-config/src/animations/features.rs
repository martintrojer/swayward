use knuffel::errors::DecodeError;

use super::*;
use crate::utils::parse_arg_node;

impl<S> knuffel::Decode<S> for WorkspaceSwitchAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().0;
        Ok(Self(Animation::decode_node(node, ctx, default, |_, _| {
            Ok(false)
        })?))
    }
}

impl<S> knuffel::Decode<S> for WindowMovementAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().0;
        Ok(Self(Animation::decode_node(node, ctx, default, |_, _| {
            Ok(false)
        })?))
    }
}

impl<S> knuffel::Decode<S> for WindowOpenAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().anim;
        let mut custom_shader = None;
        let anim = Animation::decode_node(node, ctx, default, |child, ctx| {
            if &**child.node_name == "custom-shader" {
                custom_shader = parse_arg_node("custom-shader", child, ctx)?;
                Ok(true)
            } else {
                Ok(false)
            }
        })?;

        Ok(Self {
            anim,
            custom_shader,
        })
    }
}

impl<S> knuffel::Decode<S> for WindowCloseAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().anim;
        let mut custom_shader = None;
        let anim = Animation::decode_node(node, ctx, default, |child, ctx| {
            if &**child.node_name == "custom-shader" {
                custom_shader = parse_arg_node("custom-shader", child, ctx)?;
                Ok(true)
            } else {
                Ok(false)
            }
        })?;

        Ok(Self {
            anim,
            custom_shader,
        })
    }
}

impl<S> knuffel::Decode<S> for WindowResizeAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().anim;
        let mut custom_shader = None;
        let anim = Animation::decode_node(node, ctx, default, |child, ctx| {
            if &**child.node_name == "custom-shader" {
                custom_shader = parse_arg_node("custom-shader", child, ctx)?;
                Ok(true)
            } else {
                Ok(false)
            }
        })?;

        Ok(Self {
            anim,
            custom_shader,
        })
    }
}

impl<S> knuffel::Decode<S> for ConfigNotificationOpenCloseAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().0;
        Ok(Self(Animation::decode_node(node, ctx, default, |_, _| {
            Ok(false)
        })?))
    }
}

impl<S> knuffel::Decode<S> for ExitConfirmationOpenCloseAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().0;
        Ok(Self(Animation::decode_node(node, ctx, default, |_, _| {
            Ok(false)
        })?))
    }
}

impl<S> knuffel::Decode<S> for ScreenshotUiOpenAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().0;
        Ok(Self(Animation::decode_node(node, ctx, default, |_, _| {
            Ok(false)
        })?))
    }
}

impl<S> knuffel::Decode<S> for OverviewOpenCloseAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().0;
        Ok(Self(Animation::decode_node(node, ctx, default, |_, _| {
            Ok(false)
        })?))
    }
}

impl<S> knuffel::Decode<S> for RecentWindowsCloseAnim
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let default = Self::default().0;
        Ok(Self(Animation::decode_node(node, ctx, default, |_, _| {
            Ok(false)
        })?))
    }
}
