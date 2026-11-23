use super::{Analyzer, charset_names::CHARSET_NAMES};
use crate::{LanguageVersion, ModelInfo, syntax::ModelDecl};
use std::net::Ipv6Addr;

impl Analyzer<'_, '_> {
    pub(super) fn model_info(
        &mut self,
        language: LanguageVersion,
        model: &ModelDecl,
    ) -> Option<ModelInfo> {
        let issuer = &model.issuer;
        if !self.step(issuer.value.len(), &issuer.span) {
            return None;
        }
        if issuer.value.len() > 1023 || !valid_uri_reference(&issuer.value) {
            self.diagnostics.push(
                "ILI-MODEL-ISSUER",
                "The model issuer must be an ASCII URI reference of at most 1023 characters",
                Some(issuer.span.clone()),
            );
            return None;
        }
        let charset = if let Some(label) = &model.charset {
            if !self.step(label.value.len(), &label.span) {
                return None;
            }
            let Some(canonical) = canonical_charset(&label.value) else {
                self.diagnostics.push(
                    "ILI-MODEL-CHARSET",
                    "CHARSET must name an IANA registered character set",
                    Some(label.span.clone()),
                );
                return None;
            };
            Some((label.value.clone(), canonical))
        } else {
            None
        };
        if let Some(namespace) = &model.xmlns {
            if !self.step(namespace.value.len(), &namespace.span) {
                return None;
            }
            if namespace.value.is_empty()
                || matches!(
                    namespace.value.as_str(),
                    "http://www.w3.org/2000/xmlns/" | "http://www.w3.org/XML/1998/namespace"
                )
                || !valid_uri_reference(&namespace.value)
            {
                self.diagnostics.push(
                    "ILI-MODEL-XMLNS",
                    "XMLNS requires a nonempty, unreserved URI reference",
                    Some(namespace.span.clone()),
                );
                return None;
            }
        }
        Some(ModelInfo {
            name: model.name.value.clone(),
            language,
            language_tag: model.language_tag.as_ref().map(|tag| tag.value.clone()),
            issuer: issuer.value.clone(),
            version: model.version.value.clone(),
            contracted: model.contracted.is_some(),
            kind: model.kind,
            no_incremental_transfer: model.no_incremental_transfer.is_some(),
            explanation: model
                .explanation
                .as_ref()
                .map(|explanation| explanation.value.clone()),
            charset,
            xml_namespace: model
                .xmlns
                .as_ref()
                .map(|namespace| namespace.value.clone()),
            imports: Vec::new(),
        })
    }
}

fn canonical_charset(label: &str) -> Option<&'static str> {
    let label = label.to_ascii_lowercase();
    let index = CHARSET_NAMES
        .binary_search_by_key(&label.as_str(), |&(alias, _)| alias)
        .ok()?;
    Some(CHARSET_NAMES[index].1)
}

fn valid_uri_reference(value: &str) -> bool {
    // RFC 2396 relative references and RFC 2732 IPv6 literals. Parse the
    // spelling locally, without URL normalization or host resolution.
    let (reference, fragment) = match value.split_once('#') {
        Some((reference, fragment)) => (reference, Some(fragment)),
        None => (value, None),
    };
    if fragment.is_some_and(|fragment| !uri_chars(fragment, b";/?:@&=+$,[]")) {
        return false;
    }
    let scheme_end = reference.find([':', '/', '?']);
    let hierarchy = if let Some(at) = scheme_end
        && reference.as_bytes()[at] == b':'
    {
        let mut scheme = reference[..at].bytes();
        let path = &reference[at + 1..];
        if !scheme
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            || !scheme.all(|byte| byte.is_ascii_alphanumeric() || b"+-.".contains(&byte))
            || path.is_empty()
        {
            return false;
        }
        path
    } else {
        reference
    };
    let (path, query) = match hierarchy.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (hierarchy, None),
    };
    if query.is_some_and(|query| !uri_chars(query, b";/?:@&=+$,[]")) {
        return false;
    }
    if let Some(network) = path.strip_prefix("//") {
        let (authority, path) = network.split_once('/').unwrap_or((network, ""));
        (!network.is_empty() || query.is_some())
            && authority_is_valid(authority)
            && uri_chars(path, b";/:@&=+$,")
    } else {
        uri_chars(path, b";/:@&=+$,")
    }
}

fn authority_is_valid(authority: &str) -> bool {
    let (userinfo, host) = authority.rsplit_once('@').unzip();
    let host = host.unwrap_or(authority);
    if userinfo.is_some_and(|userinfo| !uri_chars(userinfo, b";:&=+$,")) {
        return false;
    }
    if let Some(literal) = host.strip_prefix('[') {
        let Some((address, port)) = literal.split_once(']') else {
            return false;
        };
        return address.parse::<Ipv6Addr>().is_ok()
            && (port.is_empty()
                || port
                    .strip_prefix(':')
                    .is_some_and(|digits| digits.bytes().all(|byte| byte.is_ascii_digit())));
    }
    uri_chars(host, b";:&=+$,")
}

fn uri_chars(value: &str, allowed: &[u8]) -> bool {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            if !bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit())
                || !bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit())
            {
                return false;
            }
        } else if !byte.is_ascii_alphanumeric()
            && !b"-_.!~*'()".contains(&byte)
            && !allowed.contains(&byte)
        {
            return false;
        }
    }
    true
}
