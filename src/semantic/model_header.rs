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
        if issuer.value.len() > 1023 || !valid_uri_reference(&issuer.value, UriProfile::Issuer) {
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
                || !valid_uri_reference(&namespace.value, UriProfile::Namespace)
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
            effective_xml_namespace: match (&model.xmlns, language) {
                (Some(namespace), _) => namespace.value.clone(),
                (None, LanguageVersion::V23) => "http://www.interlis.ch/INTERLIS2.3".into(),
                (None, LanguageVersion::V24) => {
                    format!("http://www.interlis.ch/xtf/2.4/{}", model.name.value)
                }
            },
            span: model.span.clone(),
            issuer_span: issuer.span.clone(),
            version_span: model.version.span.clone(),
            charset_span: model.charset.as_ref().map(|label| label.span.clone()),
            xml_namespace_span: model.xmlns.as_ref().map(|namespace| namespace.span.clone()),
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum UriProfile {
    Issuer,
    Namespace,
}

fn valid_uri_reference(value: &str, profile: UriProfile) -> bool {
    // The issuer uses the RFC 2396 reference grammar. XML Namespaces 1.0
    // uses RFC 3986. Neither grammar requires normalization or resolution.
    let query_chars: &[u8] = match profile {
        UriProfile::Issuer => b";/?:@&=+$,[]",
        UriProfile::Namespace => b";/?:@&=+$,!",
    };
    let (reference, fragment) = match value.split_once('#') {
        Some((reference, fragment)) => (reference, Some(fragment)),
        None => (value, None),
    };
    if fragment.is_some_and(|fragment| !uri_chars(fragment, query_chars)) {
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
            || path.is_empty() && profile == UriProfile::Issuer
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
    if query.is_some_and(|query| !uri_chars(query, query_chars)) {
        return false;
    }
    if let Some(network) = path.strip_prefix("//") {
        let (authority, path) = network.split_once('/').unwrap_or((network, ""));
        (profile == UriProfile::Namespace
            || !network.is_empty()
            || query.is_some()
            || fragment.is_some())
            && authority_is_valid(authority, profile)
            && uri_chars(path, b";/:@&=+$,")
    } else {
        uri_chars(path, b";/:@&=+$,")
    }
}

fn authority_is_valid(authority: &str, profile: UriProfile) -> bool {
    if profile == UriProfile::Issuer && uri_chars(authority, b";:@&=+$,") {
        // RFC 2396 permits a registry-based authority as well as a server.
        return true;
    }
    let (userinfo, host) = authority.rsplit_once('@').unzip();
    let host = host.unwrap_or(authority);
    if userinfo.is_some_and(|userinfo| !uri_chars(userinfo, b";:&=+$,")) {
        return false;
    }
    if let Some(literal) = host.strip_prefix('[') {
        let Some((address, port)) = literal.split_once(']') else {
            return false;
        };
        return (address.parse::<Ipv6Addr>().is_ok()
            || profile == UriProfile::Namespace && ipv_future(address))
            && (port.is_empty()
                || port
                    .strip_prefix(':')
                    .is_some_and(|digits| digits.bytes().all(|byte| byte.is_ascii_digit())));
    }
    let (hostname, port) = host.split_once(':').unzip();
    uri_chars(hostname.unwrap_or(host), b";&=+$,")
        && port.is_none_or(|port| port.bytes().all(|byte| byte.is_ascii_digit()))
}

fn ipv_future(address: &str) -> bool {
    let Some((version, name)) = address
        .strip_prefix(['v', 'V'])
        .and_then(|address| address.split_once('.'))
    else {
        return false;
    };
    !version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !name.is_empty()
        && !name.contains('%')
        && uri_chars(name, b";:&=+$,")
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
