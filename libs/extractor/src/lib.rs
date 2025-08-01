mod component;
mod css_utils;
pub mod extract_style;
mod extractor;
mod gen_class_name;
mod gen_style;
mod prop_modify_utils;
mod util_type;
mod utils;
mod visit;
use crate::extract_style::extract_style_value::ExtractStyleValue;
use crate::visit::DevupVisitor;
use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_ast_visit::VisitMut;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::{Parser, ParserReturn};
use oxc_span::SourceType;
use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::path::PathBuf;
#[derive(Debug)]
pub enum ExtractStyleProp<'a> {
    Static(ExtractStyleValue),
    StaticArray(Vec<ExtractStyleProp<'a>>),
    Conditional {
        condition: Expression<'a>,
        consequent: Option<Box<ExtractStyleProp<'a>>>,
        alternate: Option<Box<ExtractStyleProp<'a>>>,
    },
    Expression {
        styles: Vec<ExtractStyleValue>,
        expression: Expression<'a>,
    },
    MemberExpression {
        map: BTreeMap<String, Box<ExtractStyleProp<'a>>>,
        expression: Expression<'a>,
    },
}

impl ExtractStyleProp<'_> {
    pub fn extract(&self) -> Vec<ExtractStyleValue> {
        match self {
            ExtractStyleProp::Static(style) => vec![style.clone()],
            ExtractStyleProp::Conditional {
                consequent,
                alternate,
                ..
            } => {
                let mut styles = vec![];
                if let Some(consequent) = consequent {
                    styles.append(&mut consequent.extract());
                }
                if let Some(alternate) = alternate {
                    styles.append(&mut alternate.extract());
                }
                styles
            }
            ExtractStyleProp::StaticArray(array) => {
                array.iter().flat_map(|s| s.extract()).collect()
            }
            ExtractStyleProp::Expression { styles, .. } => styles.to_vec(),
            ExtractStyleProp::MemberExpression { map, .. } => {
                map.values().flat_map(|s| s.extract()).collect()
            }
        }
    }
}
/// Style property for props
#[derive(Debug)]
pub struct ExtractOutput {
    // used styles
    pub styles: HashSet<ExtractStyleValue>,

    // output source
    pub code: String,

    pub map: Option<String>,
}

pub struct ExtractOption {
    pub package: String,
    pub css_file: Option<String>,
}

pub fn extract(
    filename: &str,
    code: &str,
    option: ExtractOption,
) -> Result<ExtractOutput, Box<dyn Error>> {
    let source_type = SourceType::from_path(filename)?;
    if !code.contains(option.package.as_str()) {
        // skip if not using package
        return Ok(ExtractOutput {
            styles: HashSet::new(),
            code: code.to_string(),
            map: None,
        });
    }
    let allocator = Allocator::default();

    let ParserReturn {
        mut program, // AST
        panicked,    // Parser encountered an error it couldn't recover from
        ..
    } = Parser::new(&allocator, code, source_type).parse();
    if panicked {
        return Err("Parser panicked".into());
    }
    let mut visitor = DevupVisitor::new(
        &allocator,
        filename,
        &option.package,
        &option
            .css_file
            .unwrap_or(format!("{}/devup-ui.css", option.package)),
    );
    visitor.visit_program(&mut program);
    let result = Codegen::new()
        .with_options(CodegenOptions {
            source_map_path: Some(PathBuf::from(filename)),
            ..Default::default()
        })
        .build(&program);

    Ok(ExtractOutput {
        styles: visitor.styles,
        code: result.code,
        map: result.map.map(|m| m.to_json_string()),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use css::class_map::reset_class_map;
    use insta::assert_debug_snapshot;
    use serial_test::serial;

    #[derive(Debug)]
    #[allow(dead_code)]
    struct ToBTreeSet {
        // used styles
        pub(crate) styles: BTreeSet<ExtractStyleValue>,

        // output source
        pub(crate) code: String,
    }

    impl From<ExtractOutput> for ToBTreeSet {
        fn from(output: ExtractOutput) -> Self {
            Self {
                styles: {
                    let mut set = BTreeSet::new();
                    set.extend(output.styles);
                    set
                },
                code: output.code,
            }
        }
    }
    #[test]
    #[serial]
    fn extract_just_tsx() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                "const a = 1;",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                },
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                "<Box gap={1} />",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                },
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn ignore_special_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box padding={1} ref={ref} data-test={1} role={2} children={[]} onClick={()=>{}} aria-valuenow={24} key={2} tabIndex={1} id="id" />
        "#,
                ExtractOption { package: "@devup-ui/core".to_string(), css_file: None }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Input} from '@devup-ui/core'
        <Input placeholder="a" maxLength="b" minLength="c" />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn convert_tag() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box as="section" />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box as={"section"} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box as={`section`} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box as={"section"}></Box>
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box as={`section`}></Box>
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        // assert_debug_snapshot!(extract(
        //     "test.tsx",
        //     r#"import {Box} from '@devup-ui/core'
        // <Box as={b ? "div":"section"} />
        // "#,
        //     ExtractOption {
        //         package: "@devup-ui/core".to_string(),
        //         css_file: None
        //     }
        // )
        // .unwrap());
        // assert_debug_snapshot!(extract(
        //     "test.tsx",
        //     r#"import {Box} from '@devup-ui/core'
        // <Box as={b ? undefined:"section"} />
        // "#,
        //     ExtractOption {
        //         package: "@devup-ui/core".to_string(),
        //         css_file: None
        //     }
        // )
        // .unwrap());
        //
        // assert_debug_snapshot!(extract(
        //     "test.tsx",
        //     r#"import {Box} from '@devup-ui/core'
        // <Box as={b ? null:"section"} />
        // "#,
        //     ExtractOption {
        //         package: "@devup-ui/core".to_string(),
        //         css_file: None
        //     }
        // )
        // .unwrap());
        // assert_debug_snapshot!(extract(
        //     "test.tsx",
        //     r#"import {Box} from '@devup-ui/core'
        // <Box as={b ? null:undefined} />
        // "#,
        //     ExtractOption {
        //         package: "@devup-ui/core".to_string(),
        //         css_file: None
        //     }
        // )
        // .unwrap());
    }
    #[test]
    #[serial]
    fn extract_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box padding={1} margin={2} wrong={} wrong2=<></> />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box as C} from '@devup-ui/core'
                <C padding={1} margin={2} />
                ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Input} from '@devup-ui/core'
        <Input padding={1} margin={2} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Button} from '@devup-ui/core'
        <Button padding={1} margin={2} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Flex} from '@devup-ui/core'
        <Flex padding={1} margin={2} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Flex} from '@devup-ui/core'
        <Flex padding={('-1')}/>
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_style_props_with_namespace_import() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import * as B from '@devup-ui/core'
        <B.Flex padding={('-1')} className={B.css({
            color: 'red'
        })}/>
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_style_props_with_var_css() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {css} from '@devup-ui/core'
        const newCss=css;
        <div className={newCss({
            color: 'red'
        })}/>
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_style_props_with_default_import() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import B from '@devup-ui/core'
        <B.Flex padding={('-1')} className={B.css({
            color: 'red'
        })}/>
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_style_props_with_class_name() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as C} from '@devup-ui/core'
        <C padding={1} margin={2} className="exists class name" />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as C} from '@devup-ui/core'
        <C padding={1} margin={2} className="  exists class name  " />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as C} from '@devup-ui/core'
        <C padding={1} margin={2} className={"exists class name"} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as C} from '@devup-ui/core'
        <C padding={1} margin={2} className={} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as C} from '@devup-ui/core'
        <C padding={1} margin={2} className={"a"+"b"} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Image} from '@devup-ui/core'
        <Image
          className={styles.logo}
          src="/next.svg"
          alt="Next.js logo"
          width={180}
          height={38}
        />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as C} from '@devup-ui/core'
        <C padding={1} margin={2} className={variable} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as C} from '@devup-ui/core'
        <C padding={1} 
      _hover={{
        borderColor: true ? 'blue' : ``,
      }}
 className={variable} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box, Button as DevupButton, Center, css } from '@devup-ui/core'
import clsx from 'clsx'

<DevupButton
      boxSizing="border-box"
      className={clsx(
        variants[variant],
        isError && variant === 'default' && errorClassNames,
        className,
      )}
      typography={
        isPrimary
          ? {
              sm: 'buttonS',
              md: 'buttonM',
            }[size]
          : undefined
      }
      {...props}
    />
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_class_name_from_component() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {VStack as C} from '@devup-ui/core'
        <C padding={1} margin={2} className={"a"+"b"} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn extract_responsive_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={[null,1]} margin={[2,null,4]} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Flex } from "@devup-ui/core";
<Flex display={['none', null, "flex"]}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_dynamic_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={someStyleVar} margin={someStyleVar2} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={Math.abs(5)} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box bg={data.buttonBgColor} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box bg={data.a.b.buttonBgColor} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_dynamic_style_props_with_type() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={a as A} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={data[d as A] as B} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={"10px" as B} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn remove_semicolon() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box bg="red;" />
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box bg="blue;;" />
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box bg={`${"green;"}`} />
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box bg={`${color};`} />
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box bg={`${color}` + ";"} />
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_dynamic_responsive_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={[someStyleVar,null,someStyleVar1]} margin={[null,someStyleVar2]} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_compound_responsive_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={[someStyleVar,undefined,someStyleVar1]} margin={[null,someStyleVar2]} bg="red" />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_wrong_responsive_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box padding={[NaN,undefined,null]} margin={Infinity} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_variable_style_props_with_style() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a} style={{ key:value }} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a} style={styles} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_conditional_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? "4px" : "3px"} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? c : d} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? "4px" : d} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? null : undefined} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? 1 : undefined} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? undefined : 2} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? `${a}px` : undefined} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? null : `${b}px`} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? `${b}px` : undefined} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_same_value_conditional_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? "4px" : "4px"} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? 4 : 4} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? `4px` : `4px`} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? `${"1px"}` : `${"1px"}`} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? `${"1"}px` : `${1}px`} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? `${`1`}px` : `${"1"}px`} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_same_dynamic_value_conditional_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? a : a} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? `${a}` : `${a}`} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_responsive_conditional_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={[null, a === b ? "4px" : "3px"]} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
        <Box margin={["6px", a === b ? "4px" : "3px"]} />;
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? c : [d, e, f, "2px"]} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? c : [d, e, f, x === y ? "4px" : "2px"]} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? [d, e, f, x === y ? "4px" : "2px"] : c} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a === b ? [d, e, f, x === y ? "4px" : "2px"] : ["1px", "2px", "3px"]} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={[null, a === b && "4px"]} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={[null, a === b && "4px", c === d ? "5px" : null]} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_logical_case() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a===b && "1px"} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a===b || "1px"} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a ?? "1px"} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={(a===1||a===2)&&b===3 && "1px"} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_dynamic_logical_case() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a===b && a} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a===b || a} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={a ?? b} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={(a===1||a===2)&&b===3 && a} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn extract_responsive_conditional_style_props_with_class_name() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={[null, a === b ? (q > w ? "4px" : "8px") : "3px"]} className={"exists"} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { Box } from "@devup-ui/core";
<Box margin={[null, a === b || "4px"]} className={"exists"} />;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_selector() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box _hover={{
          mx: 1
        }} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Center} from '@devup-ui/core'
    <Center
      _active={
        variant !== 'disabled' && {
          boxShadow: 'none',
          transform: 'scale(0.95)',
        }
      }
      _hover={
        variant !== 'disabled' && {
          boxShadow: [
            '0px 1px 3px 0px rgba(0, 0, 0, 0.25)',
            null,
            '0px 0px 15px 0px rgba(0, 0, 0, 0.25)',
          ],
        }
      }
      {...props}
    >
      {children}
    </Center>
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_selector_with_literal() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box _hover={`
        background-color: red;
        `} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box selectors={{
          "&:hover":`
          background-color: red;
          ` 
        }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn extract_nested_selector() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box _hover={{
          _placeholder: {
            color: "red"
          }
        }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box _hover={{
          selectors: {
            "&::placeholder, &:active": {
              color: "blue"
            }
          },
        }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box _hover={{
          selectors: {
            "&::placeholder": {
              color: "red"
            },
            "&::placeholder, &:active": {
              color: "blue"
            }
          },
        }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box _hover={{
          selectors: {
            "&::placeholder": {
              _active: {
                color: "red",
              }
            },
          },
        }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box 
          selectors={{
            "&::placeholder": {
              _active: {
                color: "red",
              }
            },
        }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box 
          selectors={{
            "&::placeholder": {
              selectors: {
                "&:active": {
                  selectors: {
                    "&:hover": {
                      color: "red",
                    }
                  }
                }
              }
            },
        }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box 
          _placeholder={{
            _active: {
              _hover: {
                color: "blue",
              },
              color: "red",
            },
        }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_conditional_selector() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box _hover={a===b ? undefined : {
          mx: 1
        }} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box _hover={a===b && {
          mx: 1
        }} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box _hover={a===b && {}} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box _hover={a===b && {
          mx: 1,
          my: 1
        }} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_selector_with_responsive() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box _hover={{
          mx: [1, 2]
        }} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box _hover={[{
          mx: 10
        },{
          mx: 20
        }]} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r"import {Box} from '@devup-ui/core'
        <Box _hover={[`
        margin-left: 10px;
        margin-right: 10px;
        `,{
        marginLeft: '20px',
        marginRight: '20px',
        }]} />
        ",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_static_css_class_name_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css`
  background-color: red;
`}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css as c } from "@devup-ui/core";
<Box className={c`
  background-color: red;
`}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css({
  bg:"red",
  color:"blue"
})}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css as c } from "@devup-ui/core";
<Box className={c({
  bg:"red"
})}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css({
  _hover: {
    bg:"red",
    color:"blue"
  }
})}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
        <div className={css(a?{bg:"red"}:{bg:"blue"})}/>;
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css()}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css({})}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css({
  _hover:`
  background-color: red;
  color: blue;
`
})}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css({
  _hover:[`
  background-color: red;
  color: blue;
`,{
  backgroundColor: "green",
  color: "yellow"
}, `
  background-color: red;
  color: blue;
`]
})}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css``}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css`   `}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css`  
 `}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_static_css_with_media_query() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css`
  @media (min-width: 768px) {
    & {
      background-color: red;
    }
  }
`}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css`
  @media (min-width: 768px) {
    &:hover {
      background-color: red;
    }
    &:active {
      background-color: blue;
    }
  }
`}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { css } from "@devup-ui/core";
<Box className={css`
  @media (min-width: 768px) {
    background-color: red;
  }
`}/>;
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_static_css_with_theme() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box color="$nice" />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box color={`$nice`} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box} from '@devup-ui/core'
        <Box color={("$nice")} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn apply_typography() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Text} from '@devup-ui/core'
        <Text typography="bold" />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Text} from '@devup-ui/core'
        <Text typography={`bold`} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Text} from '@devup-ui/core'
        <Text typography={a ? "bold" : "bold2"} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn apply_var_typography() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Text} from '@devup-ui/core'
        <Text typography={variable} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Text} from '@devup-ui/core'
        <Text typography={bo ? a : b} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Text} from '@devup-ui/core'
        <Text typography={`${bo ? a : b}`} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as DevupButton} from '@devup-ui/core'
        <DevupButton
      boxSizing="border-box"
      className={className}
      typography={typography}
    >
    </DevupButton>
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn raise_error() {
        reset_class_map();
        assert!(
            extract(
                "test.wrong",
                "const a = 1;",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                },
            )
            .unwrap_err()
            .to_string()
            .starts_with("Unknown file extension")
        );

        reset_class_map();
        assert_eq!(
            extract(
                "test.tsx",
                "import {} '@devup-ui/core';\na a = 1;",
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                },
            )
            .unwrap_err()
            .to_string(),
            "Parser panicked"
        );
    }

    #[test]
    #[serial]
    fn import_wrong_component() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {W} from '@devup-ui/core'
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {W, useTheme} from '@devup-ui/core';
useTheme();
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn support_transpile_mjs() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.mjs",
                r#"import { jsxs as r, jsx as e } from "react/jsx-runtime";
import { Box as o, Text as t, Flex as i } from "@devup-ui/react";
function c() {
  return /* @__PURE__ */ r("div", { children: [
    /* @__PURE__ */ e(
      o,
      {
        _hover: {
          bg: "blue"
        },
        bg: "$text",
        color: "red",
        children: "hello"
      }
    ),
    /* @__PURE__ */ e(t, { typography: "header", children: "typo" }),
    /* @__PURE__ */ e(i, { as: "section", mt: 2, children: "section" })
  ] });
}
export {
  c as Lib
};"#,
                ExtractOption {
                    package: "@devup-ui/react".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.js",
                r#"import { jsxs as r, jsx as e } from "react/jsx-runtime";
import { Box as o, Text as t, Flex as i } from "@devup-ui/react";
function c() {
  return /* @__PURE__ */ r("div", { children: [
    /* @__PURE__ */ e(
      o,
      {
        _hover: {
          bg: "blue"
        },
        bg: "$text",
        color: "red",
        children: "hello"
      }
    ),
    /* @__PURE__ */ e(t, { typography: "header", children: "typo" }),
    /* @__PURE__ */ e(i, { as: "section", mt: 2, children: "section" })
  ] });
}
export {
  c as Lib
};"#,
                ExtractOption {
                    package: "@devup-ui/react".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.js",
                r#"import { jsx as e } from "react/jsx-runtime";
import { Box as o } from "@devup-ui/core";
e(o, { className: "a", bg: "red" })
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.js",
                r#"import { jsx as e } from "react/jsx-runtime";
import { Box as o } from "@devup-ui/core";
e(o, { className: "a", bg: variable, style: { color: "blue" } })
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.js",
                r#"import { jsx as e } from "react/jsx-runtime";
import { Box as o } from "@devup-ui/core";
e(o, { className: "a", bg: variable, style: { color: "blue" }, ...props })
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn support_transpile_cjs() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(extract("test.cjs", r#""use strict";Object.defineProperty(exports,Symbol.toStringTag,{value:"Module"});const e=require("react/jsx-runtime"),r=require("@devup-ui/react");function t(){return e.jsxs("div",{children:[e.jsx(r.Box,{_hover:{bg:"blue"},bg:"$text",color:"red",children:"hello"}),e.jsx(r.Text,{typography:"header",children:"typo"}),e.jsx(r.Flex,{as:"section",mt:2,children:"section"})]})}exports.Lib=t;"#, ExtractOption { package: "@devup-ui/react".to_string(), css_file: None }).unwrap()));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(extract("test.cjs", r#""use strict";Object.defineProperty(exports,Symbol.toStringTag,{value:"Module"});const {jsx:e1, jsxs:e2}=require("react/jsx-runtime"),r=require("@devup-ui/react");function t(){return e2("div",{children:[e1(r.Box,{_hover:{bg:"blue"},bg:"$text",color:"red",children:"hello"}),e1(r.Text,{typography:"header",children:"typo"}),e1(r.Flex,{as:"section",mt:2,children:"section"})]})}exports.Lib=t;"#, ExtractOption { package: "@devup-ui/react".to_string(), css_file: None }).unwrap()));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(extract("test.js", r#""use strict";Object.defineProperty(exports,Symbol.toStringTag,{value:"Module"});const e=require("react/jsx-runtime"),r=require("@devup-ui/react");function t(){return e.jsxs("div",{children:[e.jsx(r.Box,{_hover:{bg:"blue"},bg:"$text",color:"red",children:"hello"}),e.jsx(r.Text,{typography:"header",children:"typo"}),e.jsx(r.Flex,{as:"section",mt:2,children:"section"})]})}exports.Lib=t;"#, ExtractOption { package: "@devup-ui/react".to_string(), css_file: None }).unwrap()));
    }

    #[test]
    #[serial]
    fn maintain_value() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={1} zIndex={2} fontWeight={900} scale={2} flex={1} lineHeight={1} tabSize={4} MozTabSize={4} />
        "#,
                ExtractOption { package: "@devup-ui/core".to_string(), css_file: None }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn ternary_operator_in_selector() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex _hover={a ? { bg: "red" } : undefined} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex _hover={a ? { bg: "red" } : {}} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex _hover={a ? { bg: "red",color:"blue" } : { fontWeight:"bold", color:"red" }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn test_rest_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={0.5} {...props} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import { VStack } from '@devup-ui/core'

export default function Card({
  children,
  className,
  ...props
}) {
  return (
    <VStack
      _active={{
        boxShadow: 'none',
        transform: 'scale(0.95)',
      }}
      className={className}
      {...props}
    >
      {children}
    </VStack>
  )
}

        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_wrong_direct_array_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[][0]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[1, 0.5][-10]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[1, 0.5][+10]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[1, 0.5][100]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn negative_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box zIndex={-1} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box zIndex={-a} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box zIndex={-(1+a)} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box zIndex={-1*a} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box zIndex={-(1)} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box zIndex={(-1)} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box zIndex={[(-1),-2, -(3)]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_wrong_direct_object_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box opacity={{}[1]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={{a:1, b:0.5}["wrong"]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={{a:1, b:0.5}[`wrong`]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={{a:1, b:0.5}[1]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_conditional_style_props_with_class_name() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import {Box as DevupButton} from '@devup-ui/core'
        <DevupButton
      className={className}
      typography={typography}
    >
    </DevupButton>
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[1, 0.5][a]} className="ab" />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_direct_array_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[1, 0.5][0]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[1, 0.5][a]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex bg={["$red", "$blue"][idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex bg={[`$red`, `${variable}`][idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Center} from '@devup-ui/core'
<Center
            bg={['$webBg', '$appBg', '$solutionBg'][categoryId - 1]}
          >
          </Center>
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[1, 0.5, ...some][100]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={[1, 0.5, ...some][a]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_multi_expression() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {
  Box,
  Button as DevupButton,
  Center,
  css,
} from '@devup-ui/core'

<DevupButton
    border={
    {
        primary: 'none',
        default: '1px solid var(--border, #E4E4E4)',
    }[variant]
    }
    className={className}
    px={
    {
        false: { sm: '12px', md: '16px', lg: '20px' }[size],
        true: { sm: '24px', md: '28px', lg: '32px' }[size],
    }[(!!icon).toString()]
    }
/>
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_direct_object_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={{a:1, b:0.5}["a"]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={{a:1, b:0.5, ...any}["b"]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={{a:1, b:0.5, ...any}["some"]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex bg={{a:"$red", b:"$blue"}[idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_direct_variable_object_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
        <Flex opacity={{a:1, b:0.5}[a]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
<Box bg={SOME_VAR[idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_direct_object_responsive_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
;<Flex gap={{ 0: [1, 2, 3], 1: [4, 5, 6] }[idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
;<Flex gap={{ "a": [1, 2, 3], "b": [4, 5, 6] }[idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn props_direct_variable_object_responsive_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
;<Flex gap={{ 0: [a, b, c], "1": [d, e, f] }[idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_direct_array_responsive_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
;<Flex gap={[[1, 2, 3], [4, 5, 6]][idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
;<Flex gap={[[1, 2, 3],[4, 5, 6]][idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn props_direct_variable_array_responsive_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
;<Flex gap={[[a, b, c], [d, e, f]][idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn props_direct_hybrid_responsive_select() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
;<Flex gap={[[a, 1, c], [d, e, 2]][idx]} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn test_component_in_func() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Flex} from '@devup-ui/core'
PROCESS_DATA.map(({ id, title, content }, idx) => (
          <MotionDiv key={idx}>
            <Flex alignItems="center" gap={[3, null, 5, null, 10]}>
            </Flex>
          </MotionDiv>
        ))
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn backtick_prop() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
            <Box bg={`black`} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
            <Box bg={`${variable}`} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn group_selector_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
            <Box _groupHover={{ bg: "red" }} />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn test_duplicate_style_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
            <Box bg="red" background="red" />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn avoid_same_name_component() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
import {Button} from '@devup/ui'
            ;<Box bg="red" background="red" />
            ;<Button bg="red" background="red" />
        "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn css_props_destructuring_assignment() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {css} from '@devup-ui/core'
    <div className={css({
       ...(a ? { bg: 'red' } : { bg: 'blue' }),
       ...({ p: 1 }),
     })} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {css} from '@devup-ui/core'
    <div className={css({
       ...(a ? { bg: 'red', border: "solid 1px red" } : { bg: 'blue' }),
       ...({ p: 1,m: 1 }),
     })} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn theme_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box _themeDark={{ display:"none" }} _themeLight={{ display: "flex" }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn nested_theme_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box _themeDark={{
      selectors: {
        "&:hover": {
          color: "red",
        }
      },
      _active: {
        color: "blue",
        _placeholder: {
          color: "green",
        },
      },
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn template_literal_props() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box bg={`${"red"}`} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box m={`${1}`} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box m={`${-1}`} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box m={`${1} ${2}`} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box className={`  ${1} ${2}  `} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box className={`  ${1} ${2}  `}
    _hover={{bg:"red"}}
    _themeDark={{ _hover:{bg:"black"} }}
    
     />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn theme_selector() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box _themeDark={{ _hover:{bg:"black"} }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box _hover={{bg:"white"}} _themeDark={{ _hover:{bg:"black"} }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box _hover={{bg:"white"}} _themeDark={{
        selectors: {
          '& :is(svg,img)': {
            boxSize: '100%',
            filter: 'brightness(0) invert(1)',
          },
        },
      }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn custom_selector() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box selectors={{
    "&[aria-diabled='true']": {
      opacity: 0.5
      }
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box selectors={{
    "*[aria-diabled='true'] &:hover": {
      opacity: 0.5
      }
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box selectors={{
    "*[aria-diabled='true'] &": {
      opacity: 0.5
      }
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn style_order() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box styleOrder={20} p="4" _hover={{ bg: ["red", "blue"]}} selectors={{
    "*[aria-diabled='true'] &": {
      opacity: 0.5
      }
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.mjs",
                r#"import { jsxs as r, jsx as e } from "react/jsx-runtime";
import { Box as o, Text as t, Flex as i } from "@devup-ui/react";
function c() {
  return  r("div", { children: [
     e(
      o,
      {
        _hover: {
          bg: "blue"
        },
        bg: "$text",
        color: "red",
        children: "hello",
        styleOrder: 10
      }
    ),
     e(t, { typography: "header", children: "typo", styleOrder:20 }),
     e(i, { as: "section", mt: 2, children: "section",styleOrder:30 })
  ] });
}
export {
  c as Lib
};"#,
                ExtractOption {
                    package: "@devup-ui/react".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box, css} from '@devup-ui/core'
    <Box className={css({color:"white", styleOrder:100})} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box, css} from '@devup-ui/core'
    <Box className={css({color:"white"})} styleOrder={20} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box, css} from '@devup-ui/core'
    <Box className={css({color:"white",styleOrder:30})} styleOrder={20} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box, css} from '@devup-ui/core'
    <Box styleOrder={20} p="4" _hover={{ bg: ["red", "blue"]}}
    className={css({color:"white", styleOrder:100})}

     selectors={{
    "*[aria-diabled='true'] &": {
      opacity: 0.5
      }
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box, css} from '@devup-ui/core'
<Box
        aria-disabled={false}
        bg="red"
        className={css({
          bg: 'blue',
          styleOrder: 17,
        })}
        styleOrder={3}
      />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn style_order2() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box, css} from '@devup-ui/core'
    <Box styleOrder="20" p="4" _hover={{ bg: ["red", "blue"]}}
    className={css({color:"white", styleOrder:"100"})}

     selectors={{
    "*[aria-diabled='true'] &": {
      opacity: 0.5
      }
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box, css} from '@devup-ui/core'
    <Box styleOrder={"20"} p="4" _hover={{ bg: ["red", "blue"]}}
    className={css({color:"white", styleOrder:("100")})}

     selectors={{
    "*[aria-diabled='true'] &": {
      opacity: 0.5
      }
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box, css} from '@devup-ui/core'
    <Box styleOrder={`20`} p="4" _hover={{ bg: ["red", "blue"]}}
    className={css({color:"white", styleOrder:`100`})}

     selectors={{
    "*[aria-diabled='true'] &": {
      opacity: 0.5
      }
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn style_variables() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
    <Box styleVars={{
        c: "red"
    }} />
            "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box styleVars={{
            "--d": "red"
        }} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box styleVars={{
            "--d": "red",
            "e": "blue"
        }} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box styleVars={{
            variable
        }} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box styleVars={{
            variable: true ? "red" : "blue"
        }} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box style={{ "--d": "red" }} styleVars={{
            variable: true ? "red" : "blue"
        }} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box bg={color} style={{ "--d": "red" }} styleVars={{
            variable: true ? "red" : "blue"
        }} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box styleVars={{
            ["hello"]: "red"
        }} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box styleVars={{
            [true]: "red",
            [1]: "blue",
            [variable]: "green",
            [2+2]: "yellow"
        }} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.jsx",
                r#"import {Box} from '@devup-ui/core'
        <Box styleVars={styleVars} />
                "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn style_variables_mjs() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.js",
                r#"import { jsx as e } from "react/jsx-runtime";
import { Box as o } from "@devup-ui/core";
e(o, { styleVars: { c: "yellow" } })
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_global_css() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  "div": {
    bg: "red"
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  div: {
    bg: "blue"
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  ["div"]: {
    bg: "yellow"
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  [`div`]: {
    bg: "green"
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_wrong_global_css() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
    [1]: {
        bg: "red"
    }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss()
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss(1)
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_global_css_with_selector() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  "div": {
    bg: "red",
    color: "blue",
    _hover: {
      bg: "blue",
      color: "red"
    }
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  "div": {
    bg: "red",
    color: "blue",
    _hover: {
      bg: "blue",
      color: "red"
    }
  },
  "span": {
    bg: "red",
    color: "blue",
    _hover: {
      bg: "blue",
      color: "red"
    }
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  "div": {
    bg: "red",
    color: "blue",
    _hover: {
      bg: "blue",
      color: "red"
    }
  },
  "span": {
    bg: "red",
    color: "blue",
    _hover: {
      bg: "blue",
      color: "red"
    }
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  ["div"]: {
    bg: "red",
    color: "blue",
    _hover: {
      bg: "blue",
      color: "red"
    }
  },
  ["span"]: {
    bg: "red",
    color: "blue",
    _hover: {
      bg: "blue",
      color: "red"
    }
  },
  "body[data-theme='dark']": {
    bg: "red",
    color: "blue",
    _hover: {
      bg: "blue",
      color: "red"
    }
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_global_css_with_template_literal() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
    globalCss({
      "div": `
        background-color: red
      `
    })
    "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
    globalCss({
      "div": ``
    })
    "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
    globalCss({
      "div": `     `
    })
    "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
    globalCss({
      "div": `  
         `
    })
    "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
    globalCss`
    div {
      background-color: red;
      color: blue;
    }
    `
    "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
    globalCss``
    "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
    globalCss`           `
    "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
    globalCss`         
      `
    "#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_global_css_with_imports() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  imports: ["@devup-ui/core/css/global.css"]
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  imports: ["@devup-ui/core/css/global.css", "@devup-ui/core/css/global2.css"]
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  imports: [`@devup-ui/core/css/global3.css`, `@devup-ui/core/css/global4.css`]
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_global_css_with_wrong_imports() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  imports: [1, 2, "./test.css"]
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  imports: {}
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_global_css_with_empty() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  "div": {}
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  div: {},
  span: {}
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  div: ``
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({
  div: ``,
  span: ``
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss({})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { globalCss } from "@devup-ui/core";
globalCss()
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_keyframs() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  from: { opacity: 0 },
  to: { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  "0%": { opacity: 0 },
  "50%": { opacity: 0.5 },
  "100%": { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  "0": { opacity: 0 },
  "50": { opacity: 0.5 },
  "100": { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  ["0"]: { opacity: 0 },
  ["50"]: { opacity: 0.5 },
  ["100"]: { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  [0]: { opacity: 0 },
  [50]: { opacity: 0.5 },
  [100]: { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  0: { opacity: 0 },
  50: { opacity: 0.5 },
  100: { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  [`0`]: { opacity: 0 },
  [`50`]: { opacity: 0.5 },
  [`100`]: { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  [`0%`]: { opacity: 0 },
  [`50%`]: { opacity: 0.5 },
  [`100%`]: { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";

keyframes({
  [`0`]: { opacity: 0 },
  [`50`]: { opacity: 0.5 },
  [`100`]: { opacity: 1 }
});
keyframes({
  [`0%`]: { opacity: 0 },
  [`50%`]: { opacity: 0.5 },
  [`100%`]: { opacity: 1 }
});
keyframes({
  [`1%`]: { opacity: 0 },
  [`50%`]: { opacity: 0.5 },
  [`100%`]: { opacity: 1 }
});
keyframes({
  [`0%`]: { opacity: 1 },
  [`50%`]: { opacity: 0.5 },
  [`100%`]: { opacity: 1 }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
    #[test]
    #[serial]
    fn extract_wrong_keyframs() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  from: { opacity: 0 },
  [true]: { opacity: 0.5 },
  to: { opacity: 1, color: dy }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }

    #[test]
    #[serial]
    fn extract_keyframs_literal() {
        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  from: `
  background-color: red;
  `,
  to: `
  background-color: blue;
  `
})

keyframes`
  from {
    background-color: red;
  }
  to {
    background-color: blue;
  }
`
keyframes({
  from: {
    backgroundColor: "red"
  },
  to: {
    backgroundColor: "blue"
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));

        reset_class_map();
        assert_debug_snapshot!(ToBTreeSet::from(
            extract(
                "test.tsx",
                r#"import { keyframes } from "@devup-ui/core";
keyframes({
  "0%": `
  background-color: red;
  color: blue;
  `,
  "100%": `
  background-color: blue;
  color: red;
  `
})

keyframes`
  0% {
    background-color: red;
    color: blue;
  }
  100% {
    background-color: blue;
    color: red;
  }
`
keyframes({
  "0%": {
    backgroundColor: "red",
    color: "blue"
  },
  "100%": {
    backgroundColor: "blue",
    color: "red"
  }
})
"#,
                ExtractOption {
                    package: "@devup-ui/core".to_string(),
                    css_file: None
                }
            )
            .unwrap()
        ));
    }
}
