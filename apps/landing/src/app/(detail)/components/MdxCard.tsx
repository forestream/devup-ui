import { readFile } from 'node:fs/promises'
import { join } from 'node:path'

import { Box, css, VStack } from '@devup-ui/react'
import ReactMarkdown from 'react-markdown'

import { Code } from '@/components/Code'
import { _components } from '@/mdx-components'

import Card from './Card'
import MdxCardFooter from './MdxCardFooter'

export default async function MdxCard({
  src,
  demo,
}: {
  src: string
  demo: React.ReactNode
}) {
  const content = await readFile(
    join(
      process.cwd(),
      'src/app/(detail)/components/[component]',
      src ?? 'button/demo/Variants.tsx',
    ),
    {
      encoding: 'utf-8',
    },
  )
  // extract comment
  const comment = content.match(/\/\*\*[\s\S]*?\*\//)?.[0]
  const code = content.replace('\n' + comment!, '')
  const normalizedCode = code
    .split('\n')
    .map((line) => line.replaceAll('  ', '    '))
    .join('\n')
  const normalizedComment = comment
    ?.replace(/\/\*\*|\*\//g, '')
    ?.replace(/^\s*\*\s*/gm, '')

  return (
    <Card
      className={css({
        _active: {
          transform: 'none',
        },
        _hover: {
          boxShadow: 'none',
        },
        borderRadius: '10px',
        border: '1px solid $border',
        bg: '$containerBackground',
        maxW: '100%',
        flexShrink: 0,
        cursor: 'default',
        mb: '20px',
        _lastChild: {
          mb: '0',
        },
        typography: 'bodyReg',
        color: '$text',
        whiteSpace: 'pre-wrap',
      })}
    >
      <VStack gap="30px" px="24px" py="32px">
        <Box>{demo}</Box>
        <ReactMarkdown components={_components as any}>
          {normalizedComment ?? ''}
        </ReactMarkdown>
      </VStack>
      <MdxCardFooter code={normalizedCode}>
        <Code language="typescript" value={normalizedCode} />
      </MdxCardFooter>
    </Card>
  )
}
