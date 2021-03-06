// Copyright 2012 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use core::prelude::*;

use middle::ty;
use middle::ty::canon_mode;
use middle::ty::{bound_region, br_anon, br_named, br_self, br_cap_avoid,
                 br_fresh};
use middle::ty::{ctxt, field, method};
use middle::ty::{mt, t, param_bound, param_ty};
use middle::ty::{re_bound, re_free, re_scope, re_infer, re_static, Region};
use middle::ty::{ReSkolemized, ReVar};
use middle::ty::{ty_bool, ty_bot, ty_box, ty_struct, ty_enum};
use middle::ty::{ty_err, ty_estr, ty_evec, ty_float, ty_bare_fn, ty_closure};
use middle::ty::{ty_trait, ty_int};
use middle::ty::{ty_nil, ty_opaque_box, ty_opaque_closure_ptr, ty_param};
use middle::ty::{ty_ptr, ty_rptr, ty_self, ty_tup, ty_type, ty_uniq};
use middle::ty::{ty_uint, ty_unboxed_vec, ty_infer};
use metadata::encoder;
use syntax::codemap::span;
use syntax::print::pprust;
use syntax::print::pprust::mode_to_str;
use syntax::{ast, ast_util};
use syntax::ast_map;
use syntax::abi::AbiSet;

use core::str;
use core::vec;

pub fn note_and_explain_region(cx: ctxt,
                               prefix: ~str,
                               region: ty::Region,
                               suffix: ~str) {
    match explain_region_and_span(cx, region) {
      (ref str, Some(span)) => {
        cx.sess.span_note(
            span,
            fmt!("%s%s%s", prefix, (*str), suffix));
      }
      (ref str, None) => {
        cx.sess.note(
            fmt!("%s%s%s", prefix, (*str), suffix));
      }
    }
}

/// Returns a string like "the block at 27:31" that attempts to explain a
/// lifetime in a way it might plausibly be understood.
pub fn explain_region(cx: ctxt, region: ty::Region) -> ~str {
  let (res, _) = explain_region_and_span(cx, region);
  return res;
}


pub fn explain_region_and_span(cx: ctxt, region: ty::Region)
                            -> (~str, Option<span>) {
    return match region {
      re_scope(node_id) => {
        match cx.items.find(&node_id) {
          Some(&ast_map::node_block(ref blk)) => {
            explain_span(cx, "block", blk.span)
          }
          Some(&ast_map::node_expr(expr)) => {
            match expr.node {
              ast::expr_call(*) => explain_span(cx, "call", expr.span),
              ast::expr_method_call(*) => {
                explain_span(cx, "method call", expr.span)
              },
              ast::expr_match(*) => explain_span(cx, "match", expr.span),
              _ => explain_span(cx, "expression", expr.span)
            }
          }
          Some(&ast_map::node_stmt(stmt)) => {
              explain_span(cx, "statement", stmt.span)
          }
          Some(&ast_map::node_item(it, _)) if (match it.node {
                ast::item_fn(*) => true, _ => false}) => {
              explain_span(cx, "function body", it.span)
          }
          Some(_) | None => {
            // this really should not happen
            (fmt!("unknown scope: %d.  Please report a bug.", node_id),
             None)
          }
        }
      }

      re_free(id, br) => {
        let prefix = match br {
          br_anon(idx) => fmt!("the anonymous lifetime #%u defined on",
                               idx + 1),
          br_fresh(_) => fmt!("an anonymous lifetime defined on"),
          _ => fmt!("the lifetime %s as defined on",
                    bound_region_to_str(cx, br))
        };

        match cx.items.find(&id) {
          Some(&ast_map::node_block(ref blk)) => {
            let (msg, opt_span) = explain_span(cx, "block", blk.span);
            (fmt!("%s %s", prefix, msg), opt_span)
          }
          Some(_) | None => {
            // this really should not happen
            (fmt!("%s node %d", prefix, id), None)
          }
        }
      }

      re_static => { (~"the static lifetime", None) }

      // I believe these cases should not occur (except when debugging,
      // perhaps)
      re_infer(_) | re_bound(_) => {
        (fmt!("lifetime %?", region), None)
      }
    };

    fn explain_span(cx: ctxt, heading: &str, span: span)
        -> (~str, Option<span>)
    {
        let lo = cx.sess.codemap.lookup_char_pos_adj(span.lo);
        (fmt!("the %s at %u:%u", heading,
              lo.line, lo.col.to_uint()), Some(span))
    }
}

pub fn bound_region_to_str(cx: ctxt, br: bound_region) -> ~str {
    bound_region_to_str_space(cx, "&", br)
}

pub fn bound_region_to_str_space(cx: ctxt,
                                 prefix: &str,
                                 br: bound_region)
                              -> ~str {
    if cx.sess.verbose() { return fmt!("%s%? ", prefix, br); }

    match br {
      br_named(id)         => fmt!("%s'%s ", prefix, *cx.sess.str_of(id)),
      br_self              => fmt!("%s'self ", prefix),
      br_anon(_)           => prefix.to_str(),
      br_fresh(_)          => prefix.to_str(),
      br_cap_avoid(_, br)  => bound_region_to_str_space(cx, prefix, *br)
    }
}

pub fn re_scope_id_to_str(cx: ctxt, node_id: ast::node_id) -> ~str {
    match cx.items.find(&node_id) {
      Some(&ast_map::node_block(ref blk)) => {
        fmt!("<block at %s>",
             cx.sess.codemap.span_to_str(blk.span))
      }
      Some(&ast_map::node_expr(expr)) => {
        match expr.node {
          ast::expr_call(*) => {
            fmt!("<call at %s>",
                 cx.sess.codemap.span_to_str(expr.span))
          }
          ast::expr_match(*) => {
            fmt!("<match at %s>",
                 cx.sess.codemap.span_to_str(expr.span))
          }
          ast::expr_assign_op(*) |
          ast::expr_field(*) |
          ast::expr_unary(*) |
          ast::expr_binary(*) |
          ast::expr_index(*) => {
            fmt!("<method at %s>",
                 cx.sess.codemap.span_to_str(expr.span))
          }
          _ => {
            fmt!("<expression at %s>",
                 cx.sess.codemap.span_to_str(expr.span))
          }
        }
      }
      None => {
        fmt!("<unknown-%d>", node_id)
      }
      _ => { cx.sess.bug(
          fmt!("re_scope refers to %s",
               ast_map::node_id_to_str(cx.items, node_id,
                                       cx.sess.parse_sess.interner))) }
    }
}

// In general, if you are giving a region error message,
// you should use `explain_region()` or, better yet,
// `note_and_explain_region()`
pub fn region_to_str(cx: ctxt, region: Region) -> ~str {
    region_to_str_space(cx, "&", region)
}

pub fn region_to_str_space(cx: ctxt, prefix: &str, region: Region) -> ~str {
    if cx.sess.verbose() {
        return fmt!("%s%? ", prefix, region);
    }

    // These printouts are concise.  They do not contain all the information
    // the user might want to diagnose an error, but there is basically no way
    // to fit that into a short string.  Hence the recommendation to use
    // `explain_region()` or `note_and_explain_region()`.
    match region {
        re_scope(_) => prefix.to_str(),
        re_bound(br) => bound_region_to_str_space(cx, prefix, br),
        re_free(_, br) => bound_region_to_str_space(cx, prefix, br),
        re_infer(ReSkolemized(_, br)) => {
            bound_region_to_str_space(cx, prefix, br)
        }
        re_infer(ReVar(_)) => prefix.to_str(),
        re_static => fmt!("%s'static ", prefix)
    }
}

pub fn mt_to_str(cx: ctxt, m: &mt) -> ~str {
    let mstr = match m.mutbl {
      ast::m_mutbl => "mut ",
      ast::m_imm => "",
      ast::m_const => "const "
    };
    return fmt!("%s%s", mstr, ty_to_str(cx, m.ty));
}

pub fn vstore_to_str(cx: ctxt, vs: ty::vstore) -> ~str {
    match vs {
      ty::vstore_fixed(n) => fmt!("%u", n),
      ty::vstore_uniq => ~"~",
      ty::vstore_box => ~"@",
      ty::vstore_slice(r) => region_to_str_space(cx, "&", r)
    }
}

pub fn trait_store_to_str(cx: ctxt, s: ty::TraitStore) -> ~str {
    match s {
      ty::BareTraitStore => ~"",
      ty::UniqTraitStore => ~"~",
      ty::BoxTraitStore => ~"@",
      ty::RegionTraitStore(r) => region_to_str_space(cx, "&", r)
    }
}

pub fn vstore_ty_to_str(cx: ctxt, ty: ~str, vs: ty::vstore) -> ~str {
    match vs {
      ty::vstore_fixed(_) => {
        fmt!("[%s * %s]", ty, vstore_to_str(cx, vs))
      }
      ty::vstore_slice(_) => {
        fmt!("%s %s", vstore_to_str(cx, vs), ty)
      }
      _ => fmt!("%s[%s]", vstore_to_str(cx, vs), ty)
    }
}

pub fn expr_repr(cx: ctxt, expr: @ast::expr) -> ~str {
    fmt!("expr(%d: %s)",
         expr.id,
         pprust::expr_to_str(expr, cx.sess.intr()))
}

pub fn pat_repr(cx: ctxt, pat: @ast::pat) -> ~str {
    fmt!("pat(%d: %s)",
         pat.id,
         pprust::pat_to_str(pat, cx.sess.intr()))
}

pub fn tys_to_str(cx: ctxt, ts: &[t]) -> ~str {
    let tstrs = ts.map(|t| ty_to_str(cx, *t));
    fmt!("(%s)", str::connect(tstrs, ", "))
}

pub fn bound_to_str(cx: ctxt, b: param_bound) -> ~str {
    ty::param_bound_to_str(cx, &b)
}

pub fn fn_sig_to_str(cx: ctxt, typ: &ty::FnSig) -> ~str {
    fmt!("fn%s -> %s",
         tys_to_str(cx, typ.inputs.map(|a| a.ty)),
         ty_to_str(cx, typ.output))
}

pub fn ty_to_str(cx: ctxt, typ: t) -> ~str {
    fn fn_input_to_str(cx: ctxt, input: ty::arg) -> ~str {
        let ty::arg {mode: mode, ty: ty} = input;
        let modestr = match canon_mode(cx, mode) {
          ast::infer(_) => ~"",
          ast::expl(m) => {
            if !ty::type_needs_infer(ty) &&
                m == ty::default_arg_mode_for_ty(cx, ty) {
                ~""
            } else {
                mode_to_str(ast::expl(m)) + ~":"
            }
          }
        };
        fmt!("%s%s", modestr, ty_to_str(cx, ty))
    }
    fn bare_fn_to_str(cx: ctxt,
                      purity: ast::purity,
                      abis: AbiSet,
                      ident: Option<ast::ident>,
                      sig: &ty::FnSig) -> ~str
    {
        let mut s = ~"extern ";

        s.push_str(abis.to_str());
        s.push_char(' ');

        match purity {
            ast::impure_fn => {}
            _ => {
                s.push_str(purity.to_str());
                s.push_char(' ');
            }
        };

        s.push_str("fn");

        match ident {
          Some(i) => {
              s.push_char(' ');
              s.push_str(*cx.sess.str_of(i));
          }
          _ => { }
        }

        push_sig_to_str(cx, &mut s, sig);

        return s;
    }
    fn closure_to_str(cx: ctxt, cty: &ty::ClosureTy) -> ~str
    {
        let mut s = cty.sigil.to_str();

        match (cty.sigil, cty.region) {
            (ast::ManagedSigil, ty::re_static) |
            (ast::OwnedSigil, ty::re_static) => {}

            (_, region) => {
                s.push_str(region_to_str_space(cx, "", region));
            }
        }

        match cty.purity {
            ast::impure_fn => {}
            _ => {
                s.push_str(cty.purity.to_str());
                s.push_char(' ');
            }
        };

        match cty.onceness {
            ast::Many => {}
            ast::Once => {
                s.push_str(cty.onceness.to_str());
                s.push_char(' ');
            }
        };

        s.push_str("fn");

        push_sig_to_str(cx, &mut s, &cty.sig);

        return s;
    }
    fn push_sig_to_str(cx: ctxt, s: &mut ~str, sig: &ty::FnSig) {
        s.push_char('(');
        let strs = sig.inputs.map(|a| fn_input_to_str(cx, *a));
        s.push_str(str::connect(strs, ", "));
        s.push_char(')');
        if ty::get(sig.output).sty != ty_nil {
            s.push_str(" -> ");
            if ty::type_is_bot(sig.output) {
                s.push_char('!');
            } else {
                s.push_str(ty_to_str(cx, sig.output));
            }
        }
    }
    fn method_to_str(cx: ctxt, m: method) -> ~str {
        bare_fn_to_str(cx,
                       m.fty.purity,
                       m.fty.abis,
                       Some(m.ident),
                       &m.fty.sig) + ~";"
    }
    fn field_to_str(cx: ctxt, f: field) -> ~str {
        return *cx.sess.str_of(f.ident) + ~": " + mt_to_str(cx, &f.mt);
    }

    // if there is an id, print that instead of the structural type:
    /*for ty::type_def_id(typ).each |def_id| {
        // note that this typedef cannot have type parameters
        return ast_map::path_to_str(ty::item_path(cx, *def_id),
                                    cx.sess.intr());
    }*/

    // pretty print the structural type representation:
    return match ty::get(typ).sty {
      ty_nil => ~"()",
      ty_bot => ~"!",
      ty_bool => ~"bool",
      ty_int(ast::ty_i) => ~"int",
      ty_int(ast::ty_char) => ~"char",
      ty_int(t) => ast_util::int_ty_to_str(t),
      ty_uint(ast::ty_u) => ~"uint",
      ty_uint(t) => ast_util::uint_ty_to_str(t),
      ty_float(ast::ty_f) => ~"float",
      ty_float(t) => ast_util::float_ty_to_str(t),
      ty_box(ref tm) => ~"@" + mt_to_str(cx, tm),
      ty_uniq(ref tm) => ~"~" + mt_to_str(cx, tm),
      ty_ptr(ref tm) => ~"*" + mt_to_str(cx, tm),
      ty_rptr(r, ref tm) => {
        region_to_str_space(cx, ~"&", r) + mt_to_str(cx, tm)
      }
      ty_unboxed_vec(ref tm) => { ~"unboxed_vec<" + mt_to_str(cx, tm) + ~">" }
      ty_type => ~"type",
      ty_tup(ref elems) => {
        let strs = elems.map(|elem| ty_to_str(cx, *elem));
        ~"(" + str::connect(strs, ~",") + ~")"
      }
      ty_closure(ref f) => {
          closure_to_str(cx, f)
      }
      ty_bare_fn(ref f) => {
          bare_fn_to_str(cx, f.purity, f.abis, None, &f.sig)
      }
      ty_infer(infer_ty) => infer_ty.to_str(),
      ty_err => ~"[type error]",
      ty_param(param_ty {idx: id, def_id: did}) => {
          if cx.sess.verbose() {
              fmt!("'%s:%?",
                   str::from_bytes(~[('a' as u8) + (id as u8)]),
                   did)
          } else {
              fmt!("'%s",
                   str::from_bytes(~[('a' as u8) + (id as u8)]))
          }
      }
      ty_self(*) => ~"self",
      ty_enum(did, ref substs) | ty_struct(did, ref substs) => {
        let path = ty::item_path(cx, did);
        let base = ast_map::path_to_str(path, cx.sess.intr());
        parameterized(cx, base, substs.self_r, substs.tps)
      }
      ty_trait(did, ref substs, s) => {
        let path = ty::item_path(cx, did);
        let base = ast_map::path_to_str(path, cx.sess.intr());
        let ty = parameterized(cx, base, substs.self_r, substs.tps);
        fmt!("%s%s", trait_store_to_str(cx, s), ty)
      }
      ty_evec(ref mt, vs) => {
        vstore_ty_to_str(cx, fmt!("%s", mt_to_str(cx, mt)), vs)
      }
      ty_estr(vs) => fmt!("%s%s", vstore_to_str(cx, vs), ~"str"),
      ty_opaque_box => ~"@?",
      ty_opaque_closure_ptr(ast::BorrowedSigil) => ~"closure&",
      ty_opaque_closure_ptr(ast::ManagedSigil) => ~"closure@",
      ty_opaque_closure_ptr(ast::OwnedSigil) => ~"closure~",
    }
}

pub fn parameterized(cx: ctxt,
                     base: &str,
                     self_r: Option<ty::Region>,
                     tps: &[ty::t]) -> ~str {

    let r_str = match self_r {
      None => ~"",
      Some(r) => {
        fmt!("/%s", region_to_str(cx, r))
      }
    };

    if vec::len(tps) > 0u {
        let strs = vec::map(tps, |t| ty_to_str(cx, *t));
        fmt!("%s%s<%s>", base, r_str, str::connect(strs, ","))
    } else {
        fmt!("%s%s", base, r_str)
    }
}

pub fn ty_to_short_str(cx: ctxt, typ: t) -> ~str {
    let mut s = encoder::encoded_ty(cx, typ);
    if str::len(s) >= 32u { s = str::slice(s, 0u, 32u).to_owned(); }
    return s;
}

// Local Variables:
// mode: rust
// fill-column: 78;
// indent-tabs-mode: nil
// c-basic-offset: 4
// buffer-file-coding-system: utf-8-unix
// End:
